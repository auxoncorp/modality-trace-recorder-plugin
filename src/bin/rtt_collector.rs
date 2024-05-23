use clap::Parser;
use human_bytes::human_bytes;
use modality_trace_recorder_plugin::{
    tracing::try_init_tracing_subscriber, trc_reader, Command, Interruptor, ReflectorOpts,
    TraceRecorderConfig, TraceRecorderConfigEntry, TraceRecorderOpts,
};
use probe_rs::{
    config::MemoryRegion,
    probe::{list::Lister, DebugProbeSelector, WireProtocol},
    rtt::{Rtt, ScanRegion, UpChannel},
    Core, CoreStatus, HaltReason, Permissions, RegisterValue, Session, VectorCatchCondition,
};
use ratelimit::Ratelimiter;
use simple_moving_average::{NoSumSMA, SMA};
use std::{
    fs,
    io::{self, BufReader},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use thiserror::Error;
use tracing::{debug, error, info, trace, warn};

/// Collect trace recorder streaming protocol data from an on-device RTT buffer
#[derive(Parser, Debug, Clone)]
#[clap(version)]
struct Opts {
    #[clap(flatten)]
    pub rf_opts: ReflectorOpts,

    #[clap(flatten)]
    pub tr_opts: TraceRecorderOpts,

    /// Specify a target attach timeout.
    /// When provided, the plugin will continually attempt to attach and search
    /// for a valid RTT control block anywhere in the target RAM.
    ///
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "attach-timeout",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub attach_timeout: Option<humantime::Duration>,

    /// Use the provided RTT control block address instead of scanning the target memory for it.
    #[clap(
        long,
        name = "control-block-address",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub control_block_address: Option<u32>,

    /// Extract the location in memory of the RTT control block debug symbol from an ELF file.
    #[clap(long, name = "elf-file", help_heading = "STREAMING PORT CONFIGURATION")]
    pub elf_file: Option<PathBuf>,

    /// Set a breakpoint on the address of the given symbol used to signal
    /// when to optionally configure the channel mode and start reading.
    ///
    /// Can be an absolute address or symbol name.
    #[arg(
        long,
        name = "breakpoint",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub breakpoint: Option<String>,

    /// Assume thumb mode when resolving symbols from the ELF file
    /// for breakpoint addresses.
    #[arg(
        long,
        requires = "elf-file",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub thumb: bool,

    /// Disable sending control plane commands to the target.
    /// By default, CMD_SET_ACTIVE is sent on startup and shutdown to
    /// start and stop tracing on the target.
    #[clap(
        long,
        name = "disable-control-plane",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub disable_control_plane: bool,

    /// Send a stop command before a start command to reset tracing on the target.
    #[clap(
        long,
        name = "restart",
        conflicts_with = "disable-control-plane",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub restart: bool,

    /// The RTT up (target to host) channel number to poll on (defaults to 1).
    #[clap(
        long,
        name = "up-channel",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub up_channel: Option<usize>,

    /// The RTT down (host to target) channel number to send start/stop commands on (defaults to 1).
    #[clap(
        long,
        name = "down-channel",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub down_channel: Option<usize>,

    /// Select a specific probe instead of opening the first available one.
    ///
    /// Use '--probe VID:PID' or '--probe VID:PID:Serial' if you have more than one probe with the same VID:PID.
    #[structopt(long = "probe", name = "probe", help_heading = "PROBE CONFIGURATION")]
    pub probe_selector: Option<DebugProbeSelector>,

    /// The target chip to attach to (e.g. STM32F407VE).
    #[clap(long, name = "chip", help_heading = "PROBE CONFIGURATION")]
    pub chip: Option<String>,

    /// Protocol used to connect to chip.
    /// Possible options: [swd, jtag].
    ///
    /// The default value is swd.
    #[structopt(long, name = "protocol", help_heading = "PROBE CONFIGURATION")]
    pub protocol: Option<WireProtocol>,

    /// The protocol speed in kHz.
    ///
    /// The default value is 4000.
    #[clap(long, name = "speed", help_heading = "PROBE CONFIGURATION")]
    pub speed: Option<u32>,

    /// The selected core to target.
    ///
    /// The default value is 0.
    #[clap(long, name = "core", help_heading = "PROBE CONFIGURATION")]
    pub core: Option<usize>,

    /// Reset the target on startup.
    #[clap(long, name = "reset", help_heading = "PROBE CONFIGURATION")]
    pub reset: bool,

    /// Attach to the chip under hard-reset.
    #[clap(
        long,
        name = "attach-under-reset",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub attach_under_reset: bool,

    /// Chip description YAML file path.
    /// Provides custom target descriptions based on CMSIS Pack files.
    #[clap(
        long,
        name = "chip-description-path",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub chip_description_path: Option<PathBuf>,

    /// Size of the host-side RTT buffer used to store data read off the target.
    ///
    /// The default value is 1024.
    #[clap(
        long,
        name = "rtt-reader-buffer-size",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub rtt_read_buffer_size: Option<usize>,

    /// The host-side RTT polling interval.
    /// Note that when the interface returns no data, we delay longer than this
    /// interval to prevent USB connection instability.
    ///
    /// The default value is 1ms.
    ///
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "rtt-poll-interval",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub rtt_poll_interval: Option<humantime::Duration>,

    /// Periodically log RTT metrics to stdout
    #[clap(long, name = "metrics", help_heading = "REFLECTOR CONFIGURATION")]
    pub metrics: bool,
}

#[tokio::main]
async fn main() {
    match do_main().await {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{e}");
            let mut cause = e.source();
            while let Some(err) = cause {
                eprintln!("Caused by: {err}");
                cause = err.source();
            }
            std::process::exit(exitcode::SOFTWARE);
        }
    }
}

async fn do_main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();

    try_init_tracing_subscriber()?;

    let intr = Interruptor::new();
    let intr_clone = intr.clone();
    ctrlc::set_handler(move || {
        if intr_clone.is_set() {
            let exit_code = if cfg!(target_family = "unix") {
                // 128 (fatal error signal "n") + 2 (control-c is fatal error signal 2)
                130
            } else {
                // Windows code 3221225786
                // -1073741510 == C000013A
                -1073741510
            };
            std::process::exit(exit_code);
        }

        debug!("Shutdown signal received");
        intr_clone.set();
    })?;

    let mut trc_cfg = TraceRecorderConfig::load_merge_with_opts(
        TraceRecorderConfigEntry::RttCollector,
        opts.rf_opts,
        opts.tr_opts,
        false,
    )?;

    if let Some(to) = opts.attach_timeout {
        trc_cfg.plugin.rtt_collector.attach_timeout = Some(to.into());
    }
    if let Some(addr) = opts.control_block_address {
        trc_cfg.plugin.rtt_collector.control_block_address = addr.into();
    }
    if let Some(elf_file) = opts.elf_file.as_ref() {
        trc_cfg.plugin.rtt_collector.elf_file = Some(elf_file.clone());
    }
    if let Some(breakpoint) = opts.breakpoint.as_ref() {
        trc_cfg.plugin.rtt_collector.breakpoint = Some(breakpoint.clone());
    }
    if opts.thumb {
        trc_cfg.plugin.rtt_collector.thumb = true;
    }
    if opts.disable_control_plane {
        trc_cfg.plugin.rtt_collector.disable_control_plane = true;
    }
    if opts.restart {
        trc_cfg.plugin.rtt_collector.restart = true;
    }
    if let Some(up_channel) = opts.up_channel {
        trc_cfg.plugin.rtt_collector.up_channel = up_channel;
    }
    if let Some(down_channel) = opts.down_channel {
        trc_cfg.plugin.rtt_collector.down_channel = down_channel;
    }
    if let Some(ps) = &opts.probe_selector {
        trc_cfg.plugin.rtt_collector.probe_selector = Some(ps.clone().into());
    }
    if let Some(c) = opts.chip {
        trc_cfg.plugin.rtt_collector.chip = Some(c);
    }
    if let Some(p) = opts.protocol {
        trc_cfg.plugin.rtt_collector.protocol = p;
    }
    if let Some(s) = opts.speed {
        trc_cfg.plugin.rtt_collector.speed = s;
    }
    if let Some(c) = opts.core {
        trc_cfg.plugin.rtt_collector.core = c;
    }
    if opts.reset {
        trc_cfg.plugin.rtt_collector.reset = true;
    }
    if opts.attach_under_reset {
        trc_cfg.plugin.rtt_collector.attach_under_reset = true;
    }
    if let Some(cd) = &opts.chip_description_path {
        trc_cfg.plugin.rtt_collector.chip_description_path = Some(cd.clone());
    }
    if let Some(rtt_read_buffer_size) = opts.rtt_read_buffer_size {
        trc_cfg.plugin.rtt_collector.rtt_read_buffer_size = rtt_read_buffer_size.into();
    }
    if let Some(rtt_poll_interval) = opts.rtt_poll_interval {
        trc_cfg.plugin.rtt_collector.rtt_poll_interval = Some(rtt_poll_interval.into());
    }
    if opts.metrics {
        trc_cfg.plugin.rtt_collector.metrics = true;
    }

    if trc_cfg
        .plugin
        .rtt_collector
        .rtt_read_buffer_size
        .unwrap_or(TrcRttReader::DEFAULT_RTT_BUFFER_SIZE)
        < 8
    {
        return Err(format!(
            "Invalid rtt-read-buffer-size configuration '{:?}'",
            trc_cfg.plugin.rtt_collector.rtt_read_buffer_size
        )
        .into());
    }

    let chip = trc_cfg
        .plugin
        .rtt_collector
        .chip
        .clone()
        .ok_or(Error::MissingChip)?;

    if let Some(chip_desc) = &trc_cfg.plugin.rtt_collector.chip_description_path {
        debug!(path = %chip_desc.display(), "Adding custom chip description");
        let f = fs::File::open(chip_desc)?;
        probe_rs::config::add_target_from_yaml(f)?;
    }

    let lister = Lister::new();
    let mut probe = if let Some(probe_selector) = &trc_cfg.plugin.rtt_collector.probe_selector {
        debug!(probe_selector = %probe_selector.0, "Opening selected probe");
        lister.open(probe_selector.0.clone())?
    } else {
        let probes = lister.list_all();
        debug!(probes = probes.len(), "Opening first available probe");
        if probes.is_empty() {
            return Err(Error::NoProbesAvailable.into());
        }
        probes[0].open(&lister)?
    };

    debug!(protocol = %trc_cfg.plugin.rtt_collector.protocol, speed = trc_cfg.plugin.rtt_collector.speed, "Configuring probe");
    probe.select_protocol(trc_cfg.plugin.rtt_collector.protocol)?;
    probe.set_speed(trc_cfg.plugin.rtt_collector.speed)?;

    debug!(
        chip = chip,
        core = trc_cfg.plugin.rtt_collector.core,
        "Attaching to chip"
    );
    let mut session = if trc_cfg.plugin.rtt_collector.attach_under_reset {
        probe.attach_under_reset(chip, Permissions::default())?
    } else {
        probe.attach(chip, Permissions::default())?
    };

    let rtt_scan_regions = session.target().rtt_scan_regions.clone();
    let mut rtt_scan_region = if rtt_scan_regions.is_empty() {
        ScanRegion::Ram
    } else {
        ScanRegion::Ranges(rtt_scan_regions)
    };
    if let Some(user_provided_addr) = trc_cfg.plugin.rtt_collector.control_block_address {
        debug!(
            rtt_addr = user_provided_addr,
            "Using explicit RTT control block address"
        );
        rtt_scan_region = ScanRegion::Exact(user_provided_addr);
    } else if let Some(Ok(mut file)) = trc_cfg
        .plugin
        .rtt_collector
        .elf_file
        .as_ref()
        .map(fs::File::open)
    {
        if let Some(rtt_addr) = get_rtt_symbol(&mut file) {
            debug!(rtt_addr = rtt_addr, "Found RTT symbol in ELF file");
            rtt_scan_region = ScanRegion::Exact(rtt_addr as _);
        }
    }

    let memory_map = session.target().memory_map.clone();
    let mut core = session.core(trc_cfg.plugin.rtt_collector.core)?;

    if trc_cfg.plugin.rtt_collector.reset {
        debug!("Reset and halt core");
        core.reset_and_halt(Duration::from_millis(100))?;
    }

    // Disable any previous vector catching (i.e. user just ran probe-rs run or a debugger)
    core.disable_vector_catch(VectorCatchCondition::All)?;
    core.clear_all_hw_breakpoints()?;

    if let Some(bp_sym_or_addr) = &trc_cfg.plugin.rtt_collector.breakpoint {
        let num_bp = core.available_breakpoint_units()?;

        let bp_addr = if let Some(bp_addr) = bp_sym_or_addr
            .parse::<u64>()
            .ok()
            .or(u64::from_str_radix(bp_sym_or_addr.trim_start_matches("0x"), 16).ok())
        {
            bp_addr
        } else {
            let mut file = fs::File::open(
                trc_cfg
                    .plugin
                    .rtt_collector
                    .elf_file
                    .as_ref()
                    .ok_or(Error::MissingElfFile)?,
            )?;
            let bp_addr = get_symbol(&mut file, bp_sym_or_addr)
                .ok_or_else(|| Error::ElfSymbol(bp_sym_or_addr.to_owned()))?;
            if trc_cfg.plugin.rtt_collector.thumb {
                bp_addr & !1
            } else {
                bp_addr
            }
        };

        debug!(
            available_breakpoints = num_bp,
            symbol_or_addr = bp_sym_or_addr,
            addr = format_args!("0x{:X}", bp_addr),
            "Setting breakpoint to do RTT channel setup"
        );
        core.set_hw_breakpoint(bp_addr)?;
    }

    let mut rtt = match trc_cfg.plugin.rtt_collector.attach_timeout {
        Some(to) if !to.0.is_zero() => {
            attach_retry_loop(&mut core, &memory_map, &rtt_scan_region, to.0)?
        }
        _ => {
            debug!("Attaching to RTT");
            Rtt::attach_region(&mut core, &memory_map, &rtt_scan_region)?
        }
    };

    let up_channel = rtt
        .up_channels()
        .take(trc_cfg.plugin.rtt_collector.up_channel)
        .ok_or_else(|| Error::UpChannelInvalid(trc_cfg.plugin.rtt_collector.up_channel))?;
    let up_channel_mode = up_channel.mode(&mut core)?;
    debug!(channel = up_channel.number(), mode = ?up_channel_mode, buffer_size = up_channel.buffer_size(), "Opened up channel");

    if trc_cfg.plugin.rtt_collector.reset || trc_cfg.plugin.rtt_collector.attach_under_reset {
        let sp_reg = core.stack_pointer();
        let sp: RegisterValue = core.read_core_reg(sp_reg.id())?;
        let pc_reg = core.program_counter();
        let pc: RegisterValue = core.read_core_reg(pc_reg.id())?;
        debug!(pc = %pc, sp = %sp, "Run core");
        core.run()?;
    }

    if trc_cfg.plugin.rtt_collector.breakpoint.is_some() {
        debug!("Waiting for breakpoint");
        'bp_loop: loop {
            if intr.is_set() {
                break;
            }

            match core.status()? {
                CoreStatus::Running => (),
                CoreStatus::Halted(halt_reason) => match halt_reason {
                    HaltReason::Breakpoint(_) => break 'bp_loop,
                    _ => {
                        warn!(reason = ?halt_reason, "Unexpected halt reason");
                        break 'bp_loop;
                    }
                },
                state => {
                    warn!(state = ?state, "Core is in an unexpected state");
                    break 'bp_loop;
                }
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        debug!("Run core after breakpoint setup");
        core.run()?;
    }

    if !trc_cfg.plugin.rtt_collector.disable_control_plane {
        let down_channel = rtt
            .down_channels()
            .get(trc_cfg.plugin.rtt_collector.down_channel)
            .ok_or_else(|| Error::DownChannelInvalid(trc_cfg.plugin.rtt_collector.down_channel))?;
        debug!(
            channel = down_channel.number(),
            buffer_size = down_channel.buffer_size(),
            "Opened down channel"
        );

        if trc_cfg.plugin.rtt_collector.restart {
            debug!("Sending stop command");
            let cmd = Command::Stop.to_le_bytes();
            down_channel.write(&mut core, &cmd)?;
        }

        std::thread::sleep(Duration::from_millis(200));

        debug!("Sending start command");
        let cmd = Command::Start.to_le_bytes();
        down_channel.write(&mut core, &cmd)?;
    }

    // Only hold onto the Core when we need to lock the debug probe driver (before each read/write)
    std::mem::drop(core);

    let session = Arc::new(Mutex::new(session));
    let session_clone = session.clone();
    let trc_cfg_clone = trc_cfg.clone();
    let mut join_handle: tokio::task::JoinHandle<Result<(), Error>> = tokio::spawn(async move {
        let poll_interval = trc_cfg_clone
            .plugin
            .rtt_collector
            .rtt_poll_interval
            .map(|d| d.0.into())
            .unwrap_or(TrcRttReader::DEFAULT_POLL_INTERVAL);
        let buffer_size = trc_cfg_clone
            .plugin
            .rtt_collector
            .rtt_read_buffer_size
            .unwrap_or(TrcRttReader::DEFAULT_RTT_BUFFER_SIZE);
        let metrics = if trc_cfg_clone.plugin.rtt_collector.metrics {
            Some(Metrics::new(buffer_size))
        } else {
            None
        };
        let stream = TrcRttReader::new(
            intr.clone(),
            session_clone,
            up_channel,
            trc_cfg_clone.plugin.rtt_collector.core,
            poll_interval,
            buffer_size,
            metrics,
        )?;
        let mut reader = BufReader::with_capacity(buffer_size, stream);
        trc_reader::run(&mut reader, trc_cfg_clone, intr).await?;
        Ok(())
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("User signaled shutdown");
            // Wait for any on-going transfer to complete
            let _session = session.lock().unwrap();
            std::thread::sleep(Duration::from_millis(100));
            join_handle.abort();
        }
        res = &mut join_handle => {
            match res? {
                Ok(_) => {},
                Err(e) => {
                    error!(error = %e, "Encountered and error during streaming");
                    return Err(e.into())
                }
            }
        }
    };

    let mut session = match session.lock() {
        Ok(s) => s,
        // Reader thread is either shutdown or aborted
        Err(s) => s.into_inner(),
    };

    if !trc_cfg.plugin.rtt_collector.disable_control_plane {
        let mut core = session.core(trc_cfg.plugin.rtt_collector.core)?;

        let down_channel = rtt
            .down_channels()
            .get(trc_cfg.plugin.rtt_collector.down_channel)
            .ok_or_else(|| Error::DownChannelInvalid(trc_cfg.plugin.rtt_collector.down_channel))?;
        debug!(
            channel = down_channel.number(),
            buffer_size = down_channel.buffer_size(),
            "Opened down channel"
        );

        debug!("Sending stop command");
        let cmd = Command::Stop.to_le_bytes();
        down_channel.write(&mut core, &cmd)?;
    }

    Ok(())
}

fn get_rtt_symbol<T: io::Read + io::Seek>(file: &mut T) -> Option<u64> {
    get_symbol(file, "_SEGGER_RTT")
}

fn get_symbol<T: io::Read + io::Seek>(file: &mut T, symbol: &str) -> Option<u64> {
    let mut buffer = Vec::new();
    if file.read_to_end(&mut buffer).is_ok() {
        if let Ok(binary) = goblin::elf::Elf::parse(buffer.as_slice()) {
            for sym in &binary.syms {
                if let Some(name) = binary.strtab.get_at(sym.st_name) {
                    if name == symbol {
                        return Some(sym.st_value);
                    }
                }
            }
        }
    }
    None
}

fn attach_retry_loop(
    core: &mut Core,
    memory_map: &[MemoryRegion],
    scan_region: &ScanRegion,
    timeout: humantime::Duration,
) -> Result<Rtt, Error> {
    debug!(timeout = %timeout, "Attaching to RTT");
    let timeout: Duration = timeout.into();
    let start = Instant::now();
    while Instant::now().duration_since(start) <= timeout {
        match Rtt::attach_region(core, memory_map, scan_region) {
            Ok(rtt) => return Ok(rtt),
            Err(e) => {
                if matches!(e, probe_rs::rtt::Error::ControlBlockNotFound) {
                    continue;
                }

                return Err(e.into());
            }
        }
    }

    // Timeout reached
    Ok(Rtt::attach(core, memory_map)?)
}

#[derive(Debug, Error)]
enum Error {
    #[error("No probes available")]
    NoProbesAvailable,

    #[error(
        "Missing chip. Either supply it as a option at the CLI or a config file member 'chip'"
    )]
    MissingChip,

    #[error("The RTT down channel ({0}) is invalid")]
    DownChannelInvalid(usize),

    #[error("The RTT up channel ({0}) is invalid")]
    UpChannelInvalid(usize),

    #[error("A breakpoint symbol was specified but no ELF file was provided. Specify the file path in the configuration file or command line arguments")]
    MissingElfFile,

    #[error("Could not locate the address of symbol '{0}' in the ELF file")]
    ElfSymbol(String),

    #[error("Encountered an error with the probe. {0}")]
    ProbeRs(#[from] probe_rs::Error),

    #[error("Encountered an error with the probe RTT instance. {0}")]
    ProbeRsRtt(#[from] probe_rs::rtt::Error),

    #[error("Encountered an error with the RTT rate limiter. {0}")]
    Ratelimiter(#[from] ratelimit::Error),

    #[error(transparent)]
    TraceRecorder(#[from] modality_trace_recorder_plugin::Error),
}

struct TrcRttReader {
    interruptor: Interruptor,
    session: Arc<Mutex<Session>>,
    ch: UpChannel,
    core_index: usize,
    last_poll_had_data: bool,
    poll_interval: Duration,
    ratelimiter: Ratelimiter,
    metrics: Option<Metrics>,
}

impl TrcRttReader {
    const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(1);
    const NO_DATA_POLL_INTERVAL: Duration = Duration::from_millis(100);
    const DEFAULT_RTT_BUFFER_SIZE: usize = 1024;

    pub fn new(
        interruptor: Interruptor,
        session: Arc<Mutex<Session>>,
        ch: UpChannel,
        core_index: usize,
        poll_interval: Duration,
        rtt_buffer_size: usize,
        metrics: Option<Metrics>,
    ) -> Result<Self, Error> {
        debug!(rtt_buffer_size, data_poll_interval = ?poll_interval, no_data_poll_interval = ?Self::NO_DATA_POLL_INTERVAL, "Setup RTT reader");
        let ratelimiter = Ratelimiter::builder(1, poll_interval)
            .initial_available(1)
            .build()?;
        // Make sure we can safely unwrap on set_refill_interval in the Read impl
        ratelimiter.set_refill_interval(Self::NO_DATA_POLL_INTERVAL)?;
        ratelimiter.set_refill_interval(poll_interval)?;
        Ok(Self {
            interruptor,
            session,
            ch,
            core_index,
            last_poll_had_data: true,
            poll_interval,
            ratelimiter,
            metrics,
        })
    }
}

impl io::Read for TrcRttReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.interruptor.is_set() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "RTT reader shutdown",
            ));
        }

        let mut session = self.session.lock().unwrap();
        let mut core = session
            .core(self.core_index)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut bytes_fulfilled = 0;
        while bytes_fulfilled == 0 {
            if self.interruptor.is_set() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "RTT reader shutdown",
                ));
            }

            let rtt_bytes_read = self
                .ch
                .read(&mut core, &mut buf[bytes_fulfilled..])
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            trace!(rtt_bytes_read);
            bytes_fulfilled += rtt_bytes_read;

            // NOTE: this is what probe-rs does
            //
            // Poll RTT with a frequency of 10 Hz if we do not receive any new data.
            // Once we receive new data, we bump the frequency to 1kHz (default).
            //
            // If the polling frequency is too high, the USB connection to the probe
            // can become unstable. Hence we only pull as little as necessary.
            //
            // SAFETY: we check that both intervals are valid in the constructor
            match ((rtt_bytes_read != 0), self.last_poll_had_data) {
                (true, false) => {
                    self.ratelimiter
                        .set_refill_interval(self.poll_interval)
                        .unwrap();
                }
                (false, true) => {
                    self.ratelimiter
                        .set_refill_interval(Self::NO_DATA_POLL_INTERVAL)
                        .unwrap();
                }
                _ => (),
            }
            self.last_poll_had_data = rtt_bytes_read != 0;

            if let Err(delay) = self.ratelimiter.try_wait() {
                std::thread::sleep(delay);
            }

            if let Some(metrics) = self.metrics.as_mut() {
                metrics.update(rtt_bytes_read);
            }
        }

        Ok(bytes_fulfilled)
    }
}

struct Metrics {
    rtt_buffer_size: u64,
    window_start: Instant,
    read_cnt: u64,
    bytes_read: u64,
    read_zero_cnt: u64,
    read_max_cnt: u64,
    sma: NoSumSMA<f64, f64, 8>,
}

impl Metrics {
    const WINDOW_DURATION: Duration = Duration::from_secs(2);

    fn new(rtt_buffer_size: usize) -> Self {
        Self {
            rtt_buffer_size: rtt_buffer_size as u64,
            window_start: Instant::now(),
            read_cnt: 0,
            bytes_read: 0,
            read_zero_cnt: 0,
            read_max_cnt: 0,
            sma: NoSumSMA::new(),
        }
    }

    fn reset(&mut self) {
        self.read_cnt = 0;
        self.bytes_read = 0;
        self.read_zero_cnt = 0;
        self.read_max_cnt = 0;

        self.window_start = Instant::now();
    }

    fn update(&mut self, bytes_read: usize) {
        let dur = Instant::now().duration_since(self.window_start);

        self.read_cnt += 1;
        self.bytes_read += bytes_read as u64;
        if bytes_read == 0 {
            self.read_zero_cnt += 1;
        } else if bytes_read as u64 == self.rtt_buffer_size {
            self.read_max_cnt += 1;
        } else {
            self.sma.add_sample(bytes_read as f64);
        }

        if dur >= Self::WINDOW_DURATION {
            let bytes = self.bytes_read as f64;
            let secs = dur.as_secs_f64();

            info!(
                transfer_rate = format!("{}/s", human_bytes(bytes / secs)),
                cnt = self.read_cnt,
                zero_cnt = self.read_zero_cnt,
                max_cnt = self.read_max_cnt,
                avg = self.sma.get_average(),
            );

            self.reset();
        }
    }
}

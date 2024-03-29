use clap::Parser;
use modality_trace_recorder_plugin::{
    import, import::streaming::import as import_streaming, streaming::Command,
    tracing::try_init_tracing_subscriber, Interruptor, ReflectorOpts, TraceRecorderConfig,
    TraceRecorderConfigEntry, TraceRecorderOpts,
};
use probe_rs::{
    config::MemoryRegion,
    probe::{list::Lister, DebugProbeSelector, WireProtocol},
    rtt::{Rtt, ScanRegion, UpChannel},
    Core, Permissions, Session, VectorCatchCondition,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use thiserror::Error;
use tracing::{debug, error};

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

    /// Chip description YAML file path.
    /// Provides custom target descriptions based on CMSIS Pack files.
    #[clap(
        long,
        name = "chip-description-path",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub chip_description_path: Option<PathBuf>,
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
    ctrlc::set_handler(move || {
        if intr.is_set() {
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
        intr.set();
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
    if let Some(cd) = &opts.chip_description_path {
        trc_cfg.plugin.rtt_collector.chip_description_path = Some(cd.clone());
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
    let mut session = probe.attach(chip, Permissions::default())?;

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

    let mut rtt = match trc_cfg.plugin.rtt_collector.attach_timeout {
        Some(to) if !to.0.is_zero() => {
            attach_retry_loop(&mut core, &memory_map, &rtt_scan_region, to.0)?
        }
        _ => {
            debug!("Attaching to RTT");
            Rtt::attach(&mut core, &memory_map)?
        }
    };

    if trc_cfg.plugin.rtt_collector.reset {
        debug!("Run core");
        core.run()?;
    }

    let up_channel = rtt
        .up_channels()
        .take(trc_cfg.plugin.rtt_collector.up_channel)
        .ok_or_else(|| Error::UpChannelInvalid(trc_cfg.plugin.rtt_collector.up_channel))?;
    let up_channel_mode = up_channel.mode(&mut core)?;
    debug!(channel = up_channel.number(), mode = ?up_channel_mode, buffer_size = up_channel.buffer_size(), "Opened up channel");

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
        let mut stream = TrcRttReader::new(
            session_clone,
            up_channel,
            trc_cfg_clone.plugin.rtt_collector.core,
        );
        import_streaming(&mut stream, trc_cfg_clone).await?;
        Ok(())
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("User signaled shutdown");
            // Wait for any on-going transfer to complete
            let _session = session.lock().unwrap();
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

    #[error("Encountered an error with the probe. {0}")]
    ProbeRs(#[from] probe_rs::Error),

    #[error("Encountered an error with the probe RTT instance. {0}")]
    ProbeRsRtt(#[from] probe_rs::rtt::Error),

    #[error(transparent)]
    Import(#[from] import::Error),
}

struct TrcRttReader {
    session: Arc<Mutex<Session>>,
    ch: UpChannel,
    core_index: usize,
}

impl TrcRttReader {
    pub fn new(session: Arc<Mutex<Session>>, ch: UpChannel, core_index: usize) -> Self {
        Self {
            session,
            ch,
            core_index,
        }
    }
}

impl io::Read for TrcRttReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut session = self.session.lock().unwrap();
        let mut core = session
            .core(self.core_index)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut bytes_fulfilled = 0;
        while bytes_fulfilled < buf.len() {
            let rtt_bytes_read = self
                .ch
                .read(&mut core, &mut buf[bytes_fulfilled..])
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            bytes_fulfilled += rtt_bytes_read;

            // NOTE: this is what probe-rs does
            //
            // Poll RTT with a frequency of 10 Hz if we do not receive any new data.
            // Once we receive new data, we bump the frequency to 1kHz.
            //
            // If the polling frequency is too high, the USB connection to the probe
            // can become unstable. Hence we only pull as little as necessary.
            if rtt_bytes_read != 0 {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        Ok(bytes_fulfilled)
    }
}

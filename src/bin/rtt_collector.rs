use clap::Parser;
use modality_trace_recorder_plugin::{
    import, import::streaming::import as import_streaming, streaming::Command,
    tracing::try_init_tracing_subscriber, Interruptor, ReflectorOpts, TraceRecorderConfig,
    TraceRecorderConfigEntry, TraceRecorderOpts,
};
use probe_rs::{
    config::MemoryRegion, Core, DebugProbeSelector, Permissions, Probe, Session, WireProtocol,
};
use probe_rs_rtt::{Rtt, UpChannel};
use std::{
    io,
    time::{Duration, Instant},
};
use thiserror::Error;
use tracing::debug;

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
        } else {
            intr.set();
        }
    })?;

    let mut trc_cfg = TraceRecorderConfig::load_merge_with_opts(
        TraceRecorderConfigEntry::RttCollector,
        opts.rf_opts,
        opts.tr_opts,
    )?;

    if let Some(to) = opts.attach_timeout {
        trc_cfg.plugin.rtt_collector.attach_timeout = Some(to.into());
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

    let chip = trc_cfg
        .plugin
        .rtt_collector
        .chip
        .clone()
        .ok_or(Error::MissingChip)?;

    let mut probe = if let Some(probe_selector) = &trc_cfg.plugin.rtt_collector.probe_selector {
        debug!(probe_selector = %probe_selector.0, "Opening selected probe");
        Probe::open(probe_selector.0.clone())?
    } else {
        debug!("Opening first available probe");
        let probes = Probe::list_all();
        if probes.is_empty() {
            return Err(Error::NoProbesAvailable.into());
        }
        probes[0].open()?
    };

    debug!(protocol = %trc_cfg.plugin.rtt_collector.protocol, speed = trc_cfg.plugin.rtt_collector.speed, "Configuring probe");
    probe.select_protocol(trc_cfg.plugin.rtt_collector.protocol)?;
    probe.set_speed(trc_cfg.plugin.rtt_collector.speed)?;

    if trc_cfg.plugin.rtt_collector.reset {
        debug!("Reseting target");
        probe.target_reset_assert()?;
        probe.target_reset_deassert()?;
    }

    debug!(
        chip = chip,
        core = trc_cfg.plugin.rtt_collector.core,
        "Attaching to chip"
    );
    let mut session = probe.attach(chip, Permissions::default())?;

    let memory_map = session.target().memory_map.clone();
    let mut core = session.core(trc_cfg.plugin.rtt_collector.core)?;

    let mut rtt = match trc_cfg.plugin.rtt_collector.attach_timeout {
        Some(to) if !to.0.is_zero() => attach_retry_loop(&mut core, &memory_map, to.0)?,
        _ => {
            debug!("Attaching to RTT");
            Rtt::attach(&mut core, &memory_map)?
        }
    };

    let up_channel = rtt
        .up_channels()
        .take(trc_cfg.plugin.rtt_collector.up_channel)
        .ok_or_else(|| Error::UpChannelInvalid(trc_cfg.plugin.rtt_collector.up_channel))?;
    let up_channel_mode = up_channel.mode(&mut core)?;
    debug!(channel = up_channel.number(), mode = ?up_channel_mode, buffer_size = up_channel.buffer_size(), "Opened up channel");

    if !trc_cfg.plugin.rtt_collector.disable_control_plane {
        let down_channel = rtt
            .down_channels()
            .take(trc_cfg.plugin.rtt_collector.down_channel)
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

    let mut join_handle: tokio::task::JoinHandle<Result<(), Error>> = tokio::spawn(async move {
        let mut stream = TrcRttReader::new(session, up_channel, trc_cfg.plugin.rtt_collector.core);
        import_streaming(&mut stream, trc_cfg).await?;
        Ok(())
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("User signaled shutdown");
        }
        res = &mut join_handle => {
            match res? {
                Ok(_) => {},
                Err(e) => return Err(e.into()),
            }
        }
    };

    Ok(())
}

fn attach_retry_loop(
    core: &mut Core,
    memory_map: &[MemoryRegion],
    timeout: humantime::Duration,
) -> Result<Rtt, Error> {
    debug!(timeout = %timeout, "Attaching to RTT");
    let timeout: Duration = timeout.into();
    let start = Instant::now();
    while Instant::now().duration_since(start) <= timeout {
        match Rtt::attach(core, memory_map) {
            Ok(rtt) => return Ok(rtt),
            Err(e) => {
                if matches!(e, probe_rs_rtt::Error::ControlBlockNotFound) {
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
    ProbeRsRtt(#[from] probe_rs_rtt::Error),

    #[error(transparent)]
    Import(#[from] import::Error),
}

struct TrcRttReader {
    session: Session,
    ch: UpChannel,
    core_index: usize,
}

impl TrcRttReader {
    pub fn new(session: Session, ch: UpChannel, core_index: usize) -> Self {
        Self {
            session,
            ch,
            core_index,
        }
    }
}

impl io::Read for TrcRttReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut core = self
            .session
            .core(self.core_index)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut bytes_fulfilled = 0;
        while bytes_fulfilled < buf.len() {
            let rtt_bytes_read = self
                .ch
                .read(&mut core, &mut buf[bytes_fulfilled..])
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            bytes_fulfilled += rtt_bytes_read;
        }

        Ok(bytes_fulfilled)
    }
}

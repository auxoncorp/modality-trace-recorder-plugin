use auxon_sdk::reflector_config::AttrKeyEqValuePair;
use clap::Parser;
use modality_trace_recorder_plugin::{
    tracing::try_init_tracing_subscriber, trc_reader, Interruptor, ReflectorOpts, TimelineAttrKey,
    TraceRecorderConfig, TraceRecorderConfigEntry, TraceRecorderOpts,
};
use probe_rs::probe::{DebugProbeSelector, WireProtocol};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{debug, trace};
use url::Url;

/// Collect trace recorder streaming protocol data from the proxy service
#[derive(Parser, Debug, Clone)]
#[clap(version)]
pub struct Opts {
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
        value_parser=clap_num::maybe_hex::<u32>,
        name = "control-block-address",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub control_block_address: Option<u32>,

    /// Extract the location in memory of the RTT control block debug symbol from an ELF file.
    #[clap(
        long,
        env = "MODALITY_TRACE_RECORDER_ELF_FILE",
        name = "elf-file",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
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

    /// Set a breakpoint on the address of the given symbol
    /// to signal a stopping condition.
    ///
    /// Can be an absolute address (decimal or hex) or symbol name.
    #[arg(
        long,
        name = "stop-on-breakpoint",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub stop_on_breakpoint: Option<String>,

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

    /// The host-side RTT idle polling interval.
    ///
    /// The default value is 100ms.
    ///
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "rtt-idle-poll-interval",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub rtt_idle_poll_interval: Option<humantime::Duration>,

    /// Force exclusive access to the probe.
    /// Any existing sessions using this probe will be shut down.
    #[clap(
        long,
        name = "force-exclusive",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub force_exclusive: bool,

    /// Automatically attempt to recover the debug probe connection
    /// when an error is encountered
    #[clap(long, name = "auto-recover", help_heading = "REFLECTOR CONFIGURATION")]
    pub auto_recover: bool,

    /// Automatically stop the RTT session if no data is received
    /// within specified timeout duration.
    ///
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "no-data-timeout",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub no_data_timeout: Option<humantime::Duration>,

    /// Specify a connection timeout.
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "connect-timeout",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub connect_timeout: Option<humantime::Duration>,

    /// The remote TCP proxy server URL or address:port to connect to.
    ///
    /// The default is `127.0.0.1:8888`.
    #[clap(
        long,
        name = "remote",
        env = "MODALITY_TRACE_RECORDER_REMOTE",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub remote: Option<String>,
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
        } else {
            intr_clone.set();
        }
    })?;

    let mut cfg = TraceRecorderConfig::load_merge_with_opts(
        TraceRecorderConfigEntry::ProxyCollector,
        opts.rf_opts,
        opts.tr_opts,
        false,
    )?;
    if let Some(to) = opts.connect_timeout {
        cfg.plugin.proxy_collector.connect_timeout = Some(to.into());
    }
    if let Some(to) = opts.no_data_timeout {
        cfg.plugin.proxy_collector.no_data_stop_timeout = Some(to.into());
    }
    if let Some(remote) = opts.remote {
        cfg.plugin.proxy_collector.remote = Some(remote);
    }
    if let Some(to) = opts.attach_timeout {
        cfg.plugin.proxy_collector.rtt.attach_timeout = Some(to.into());
    }
    if let Some(addr) = opts.control_block_address {
        cfg.plugin.proxy_collector.rtt.control_block_address = addr.into();
    }
    if let Some(elf_file) = opts.elf_file.as_ref() {
        cfg.plugin.proxy_collector.rtt.elf_file = Some(elf_file.clone());
    }
    if let Some(breakpoint) = opts.breakpoint.as_ref() {
        cfg.plugin.proxy_collector.rtt.breakpoint = Some(breakpoint.clone());
    }
    if let Some(breakpoint) = opts.stop_on_breakpoint.as_ref() {
        cfg.plugin.proxy_collector.stop_on_breakpoint = Some(breakpoint.clone());
    }
    if opts.thumb {
        cfg.plugin.proxy_collector.rtt.thumb = true;
    }
    if opts.disable_control_plane {
        cfg.plugin.proxy_collector.rtt.disable_control_plane = true;
    }
    if opts.restart {
        cfg.plugin.proxy_collector.rtt.restart = true;
    }
    if let Some(up_channel) = opts.up_channel {
        cfg.plugin.proxy_collector.rtt.up_channel = up_channel;
    }
    if let Some(down_channel) = opts.down_channel {
        cfg.plugin.proxy_collector.rtt.down_channel = down_channel;
    }
    if let Some(ps) = &opts.probe_selector {
        cfg.plugin.proxy_collector.rtt.probe_selector = Some(ps.clone().into());
    }
    if let Some(c) = opts.chip {
        cfg.plugin.proxy_collector.rtt.chip = Some(c);
    }
    if let Some(p) = opts.protocol {
        cfg.plugin.proxy_collector.rtt.protocol = p;
    }
    if let Some(s) = opts.speed {
        cfg.plugin.proxy_collector.rtt.speed = s;
    }
    if let Some(c) = opts.core {
        cfg.plugin.proxy_collector.rtt.core = c;
    }
    if opts.reset {
        cfg.plugin.proxy_collector.rtt.reset = true;
    }
    if opts.attach_under_reset {
        cfg.plugin.proxy_collector.rtt.attach_under_reset = true;
    }
    if let Some(rtt_read_buffer_size) = opts.rtt_read_buffer_size {
        cfg.plugin.proxy_collector.rtt.rtt_read_buffer_size = rtt_read_buffer_size.into();
    }
    if let Some(rtt_poll_interval) = opts.rtt_poll_interval {
        cfg.plugin.proxy_collector.rtt.rtt_poll_interval = Some(rtt_poll_interval.into());
    }
    if let Some(rtt_idle_poll_interval) = opts.rtt_idle_poll_interval {
        cfg.plugin.proxy_collector.rtt_idle_poll_interval = Some(rtt_idle_poll_interval.into());
    }
    if opts.force_exclusive {
        cfg.plugin.proxy_collector.force_exclusive = true;
    }
    if opts.auto_recover {
        cfg.plugin.proxy_collector.auto_recover = true;
    }

    let remote_string = if let Some(remote) = cfg.plugin.proxy_collector.remote.as_ref() {
        remote.clone()
    } else {
        "127.0.0.1:8888".to_string()
    };

    let remote = if let Ok(socket_addr) = remote_string.parse::<SocketAddr>() {
        socket_addr
    } else {
        let url = Url::parse(&remote_string)
            .map_err(|e| format!("Failed to parse remote '{}' as URL. {}", remote_string, e))?;
        debug!(remote_url = %url);
        let socket_addrs = url
            .socket_addrs(|| None)
            .map_err(|e| format!("Failed to resolve remote URL '{}'. {}", url, e))?;
        *socket_addrs
            .first()
            .ok_or_else(|| format!("Could not resolve URL '{}'", url))?
    };

    let maybe_control_block_address =
        if let Some(user_provided_addr) = cfg.plugin.proxy_collector.rtt.control_block_address {
            debug!(
                rtt_addr = user_provided_addr,
                "Using explicit RTT control block address"
            );
            Some(user_provided_addr as u64)
        } else if let Some(elf_file) = &opts.elf_file {
            debug!(elf_file = %elf_file.display(), "Reading ELF file");
            let mut file = File::open(elf_file)?;
            if let Some(rtt_addr) = get_rtt_symbol(&mut file) {
                debug!(rtt_addr = rtt_addr, "Found RTT symbol");
                Some(rtt_addr)
            } else {
                debug!("Could not find RTT symbol in ELF file");
                None
            }
        } else {
            None
        };

    let maybe_setup_on_breakpoint_address =
        if let Some(bp_sym_or_addr) = &cfg.plugin.proxy_collector.rtt.breakpoint {
            if let Some(bp_addr) = bp_sym_or_addr.parse::<u64>().ok().or(u64::from_str_radix(
                bp_sym_or_addr.trim_start_matches("0x"),
                16,
            )
            .ok())
            {
                Some(bp_addr)
            } else {
                let elf_file = opts.elf_file.as_ref().ok_or_else(|| {
                    "Using a breakpoint symbol name requires an ELF file".to_owned()
                })?;
                let mut file = File::open(elf_file)?;
                let bp_addr = get_symbol(&mut file, bp_sym_or_addr).ok_or_else(|| {
                    format!(
                        "Could not locate the address of symbol '{0}' in the ELF file",
                        bp_sym_or_addr
                    )
                })?;
                if opts.thumb {
                    Some(bp_addr & !1)
                } else {
                    Some(bp_addr)
                }
            }
        } else {
            None
        };

    let maybe_stop_on_breakpoint_address =
        if let Some(bp_sym_or_addr) = &cfg.plugin.proxy_collector.stop_on_breakpoint {
            if let Some(bp_addr) = bp_sym_or_addr.parse::<u64>().ok().or(u64::from_str_radix(
                bp_sym_or_addr.trim_start_matches("0x"),
                16,
            )
            .ok())
            {
                Some(bp_addr)
            } else {
                let elf_file = opts.elf_file.as_ref().ok_or_else(|| {
                    "Using a breakpoint symbol name requires an ELF file".to_owned()
                })?;
                let mut file = File::open(elf_file)?;
                let bp_addr = get_symbol(&mut file, bp_sym_or_addr).ok_or_else(|| {
                    format!(
                        "Could not locate the address of symbol '{0}' in the ELF file",
                        bp_sym_or_addr
                    )
                })?;
                if opts.thumb {
                    Some(bp_addr & !1)
                } else {
                    Some(bp_addr)
                }
            }
        } else {
            None
        };

    let proxy_cfg = rtt_proxy::ProxySessionConfig {
        version: rtt_proxy::V1,
        probe: rtt_proxy::ProbeConfig {
            probe_selector: cfg
                .plugin
                .proxy_collector
                .rtt
                .probe_selector
                .clone()
                .map(|s| s.to_string()),
            protocol: cfg.plugin.proxy_collector.rtt.protocol.to_string(),
            speed_khz: cfg.plugin.proxy_collector.rtt.speed,
            target: cfg
                .plugin
                .proxy_collector
                .rtt
                .chip
                .clone()
                .map(rtt_proxy::Target::Specific)
                .unwrap_or(rtt_proxy::Target::Auto),
            attach_under_reset: cfg.plugin.proxy_collector.rtt.attach_under_reset,
            force_exclusive: cfg.plugin.proxy_collector.force_exclusive,
        },
        target: rtt_proxy::TargetConfig {
            auto_recover: cfg.plugin.proxy_collector.auto_recover,
            core: cfg.plugin.proxy_collector.rtt.core as _,
            reset: cfg.plugin.proxy_collector.rtt.reset,
        },
        rtt: rtt_proxy::RttConfig {
            attach_timeout_ms: cfg
                .plugin
                .proxy_collector
                .rtt
                .attach_timeout
                .map(|t| t.as_millis() as _),
            setup_on_breakpoint_address: maybe_setup_on_breakpoint_address,
            stop_on_breakpoint_address: maybe_stop_on_breakpoint_address,
            control_block_address: maybe_control_block_address,
            no_data_stop_timeout_ms: cfg
                .plugin
                .proxy_collector
                .no_data_stop_timeout
                .map(|t| t.as_millis() as _),
            up_channel: cfg.plugin.proxy_collector.rtt.up_channel as _,
            down_channel: cfg.plugin.proxy_collector.rtt.down_channel as _,
            disable_control_plane: cfg.plugin.proxy_collector.rtt.disable_control_plane,
            restart: cfg.plugin.proxy_collector.rtt.restart,
            rtt_read_buffer_size: cfg
                .plugin
                .proxy_collector
                .rtt
                .rtt_read_buffer_size
                .map(|s| s as _)
                .unwrap_or(1024),
            rtt_poll_interval_ms: cfg
                .plugin
                .proxy_collector
                .rtt
                .rtt_poll_interval
                .map(|t| t.as_millis() as _)
                .unwrap_or(100),
            rtt_idle_poll_interval_ms: cfg
                .plugin
                .proxy_collector
                .rtt_idle_poll_interval
                .map(|t| t.as_millis() as _)
                .unwrap_or(1),
        },
    };
    trace!(?proxy_cfg);

    cfg.ingest
        .timeline_attributes
        .additional_timeline_attributes
        .push(AttrKeyEqValuePair(
            TimelineAttrKey::TcpRemote.into_cfg_attr(),
            remote.to_string().into(),
        ));

    let mut stream = match cfg.plugin.proxy_collector.connect_timeout {
        Some(to) if !to.0.is_zero() => connect_retry_loop(&remote, to.0.into())?,
        _ => {
            debug!(remote = %remote, "Connecting to to remote");
            TcpStream::connect(remote)?
        }
    };

    debug!("Starting a new session");
    let mut se = serde_json::Serializer::new(&mut stream);
    proxy_cfg.serialize(&mut se)?;

    // Read response
    let mut de = serde_json::Deserializer::from_reader(&mut stream);
    let status = rtt_proxy::ProxySessionStatus::deserialize(&mut de)?;

    match status {
        rtt_proxy::ProxySessionStatus::Started(id) => debug!(%id, "Session started"),
        rtt_proxy::ProxySessionStatus::Error(e) => return Err(e.into()),
    }

    stream.set_nodelay(true)?;

    let mut join_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        trc_reader::run(&mut reader, cfg, intr).await
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("User signaled shutdown");
            std::thread::sleep(Duration::from_millis(100));
            join_handle.abort();
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

fn connect_retry_loop(
    remote: &SocketAddr,
    timeout: Duration,
) -> Result<TcpStream, Box<dyn std::error::Error>> {
    debug!(remote = %remote, timeout = ?timeout, "Connecting to to remote");
    let start = Instant::now();
    while Instant::now().duration_since(start) <= timeout {
        match TcpStream::connect_timeout(remote, timeout) {
            Ok(s) => return Ok(s),
            Err(_e) => {
                continue;
            }
        }
    }
    Ok(TcpStream::connect_timeout(remote, timeout)?)
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

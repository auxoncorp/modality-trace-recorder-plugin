#![deny(warnings, clippy::all)]

use clap::Parser;
use modality_trace_recorder_plugin::{
    import::import_streaming, streaming::Command, tracing::try_init_tracing_subscriber,
    Interruptor, ReflectorOpts, TraceRecorderConfig, TraceRecorderOpts,
};
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use tracing::debug;

/// Collect trace recorder streaming protocol data from a TCP connection
#[derive(Parser, Debug, Clone)]
pub struct Opts {
    #[clap(flatten)]
    pub rf_opts: ReflectorOpts,

    #[clap(flatten)]
    pub tr_opts: TraceRecorderOpts,

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

    /// Specify a connection timeout.
    /// Accepts durations like "10ms" or "1minute 2seconds 22ms".
    #[clap(
        long,
        name = "connect-timeout",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub connect_timeout: Option<humantime::Duration>,

    /// The remote address and port to connect to.
    #[clap(
        long,
        name = "remote",
        default_value = "127.0.0.1:8888",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub remote: SocketAddr,
}

#[tokio::main]
async fn main() {
    match do_main().await {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{}", e);
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

    let mut stream = match opts.connect_timeout {
        Some(to) if !to.is_zero() => {
            debug!(remote = %opts.remote, timeout = %to, "Connecting to to remote");
            TcpStream::connect_timeout(&opts.remote, to.into())?
        }
        _ => {
            debug!(remote = %opts.remote, "Connecting to to remote");
            TcpStream::connect(opts.remote)?
        }
    };

    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    stream.set_nodelay(true)?;
    stream.set_nonblocking(false)?;

    if !opts.disable_control_plane {
        if opts.restart {
            stream.write_all(&Command::Stop.to_le_bytes())?;
        }
        stream.write_all(&Command::Start.to_le_bytes())?;
    }

    let config = TraceRecorderConfig::from((opts.rf_opts, opts.tr_opts));
    let mut stream_clone = stream.try_clone()?;
    let mut join_handle =
        tokio::spawn(async move { import_streaming(&mut stream_clone, config).await });

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

    if !opts.disable_control_plane {
        stream.write_all(&Command::Stop.to_le_bytes())?;
    }

    Ok(())
}

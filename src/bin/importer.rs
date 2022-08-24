#![deny(warnings, clippy::all)]

use clap::Parser;
use modality_trace_recorder_plugin::{
    import::import, tracing::try_init_tracing_subscriber, ImportProtocol, Interruptor,
    ReflectorOpts, SnapshotFile, TraceRecorderConfig, TraceRecorderOpts,
};
use std::fs::File;
use std::path::PathBuf;
use tracing::debug;

/// Import trace recorder data from a file
#[derive(Parser, Debug, Clone)]
#[clap(version)]
pub struct Opts {
    #[clap(flatten)]
    pub rf_opts: ReflectorOpts,

    #[clap(flatten)]
    pub tr_opts: TraceRecorderOpts,

    /// Use the snapshot protocol instead of automatically detecting which protocol to use
    #[clap(
        long,
        name = "snapshot-protocol",
        conflicts_with = "streaming-protocol",
        help_heading = "PROTOCOL CONFIGURATION"
    )]
    pub snapshot: bool,

    /// Use the streaming protocol instead of automatically detecting which protocol to use
    #[clap(
        long,
        name = "streaming-protocol",
        conflicts_with = "snapshot-protocol",
        help_heading = "PROTOCOL CONFIGURATION"
    )]
    pub streaming: bool,

    /// Path to file
    #[clap(
        value_parser,
        name = "file path",
        help_heading = "PROTOCOL CONFIGURATION"
    )]
    pub path: PathBuf,
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

    let protocol = if opts.snapshot {
        ImportProtocol::Snapshot
    } else if opts.streaming {
        ImportProtocol::Streaming
    } else {
        ImportProtocol::Auto
    };

    let config = TraceRecorderConfig::from((opts.rf_opts, opts.tr_opts));

    let f = SnapshotFile::open(&opts.path)?;
    let mut join_handle =
        tokio::spawn(async move { import(File::from(f), protocol, config).await });

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

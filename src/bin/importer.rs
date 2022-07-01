#![deny(warnings, clippy::all)]

use clap::Parser;
use modality_trace_recorder_plugin::{
    import::{import, Config},
    CommonOpts, Interruptor, SnapshotFile,
};
use std::fs::File;
use std::path::PathBuf;
use tracing::debug;

/// Import trace recorder snapshot data from a memory dump file
#[derive(Parser, Debug, Clone)]
#[clap(version, about, long_about = None)]
pub struct Opts {
    #[clap(flatten)]
    pub common: CommonOpts,

    /// Use a single timeline for all tasks instead of a timeline per task.
    /// ISRs can still be represented with their own timelines or not.
    #[clap(long)]
    pub single_task_timeline: bool,

    /// Represent ISR in the parent task context timeline rather than a dedicated ISR timeline
    #[clap(long)]
    pub flatten_isr_timelines: bool,

    /// Use the provided initial startup task name instead of the default ('(startup)')
    #[clap(long)]
    pub startup_task_name: Option<String>,

    /// Path to memory dump file
    #[clap(value_parser)]
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

    let config = Config {
        common: opts.common,
        single_task_timeline: opts.single_task_timeline,
        flatten_isr_timelines: opts.flatten_isr_timelines,
        startup_task_name: opts.startup_task_name,
    };

    let f = SnapshotFile::open(&opts.path)?;
    let mut join_handle = tokio::spawn(async move { import(File::from(f), config).await });

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

fn try_init_tracing_subscriber() -> Result<(), Box<dyn std::error::Error>> {
    let builder = tracing_subscriber::fmt::Subscriber::builder();
    let env_filter = std::env::var(tracing_subscriber::EnvFilter::DEFAULT_ENV)
        .map(tracing_subscriber::EnvFilter::new)
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(format!(
                "{}={}",
                env!("CARGO_PKG_NAME").replace('-', "_"),
                tracing::Level::WARN
            ))
        });
    let builder = builder.with_env_filter(env_filter);
    let subscriber = builder.finish();
    use tracing_subscriber::util::SubscriberInitExt;
    subscriber.try_init()?;
    Ok(())
}

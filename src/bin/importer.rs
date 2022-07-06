#![deny(warnings, clippy::all)]

use clap::Parser;
use modality_trace_recorder_plugin::{
    import::{import, Config},
    CommonOpts, Interruptor, RenameMapItem, SnapshotFile,
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

    /// Instead of 'USER_EVENT @ <task-name>', use the user event channel
    /// as the event name (<channel> @ <task-name>)
    #[clap(
        long,
        name = "user-event-channel",
        conflicts_with = "user-event-format-string"
    )]
    pub user_event_channel: bool,

    /// Instead of 'USER_EVENT @ <task-name>', use the user event format string
    /// as the event name (<format-string> @ <task-name>)
    #[clap(
        long,
        name = "user-event-format-string",
        conflicts_with = "user-event-channel"
    )]
    pub user_event_format_string: bool,

    /// Use a custom event name whenever a user event with a matching
    /// channel is processed.
    /// Can be supplied multiple times.
    ///
    /// Format is '<input-channel>:<output-event-name>'.
    #[clap(long, name = "input-channel>:<output-event-name")]
    pub user_event_channel_name: Vec<RenameMapItem>,

    /// Use a custom event name whenever a user event with a matching
    /// formatted string is processed.
    /// Can be supplied multiple times.
    ///
    /// Format is '<input-formatted-string>:<output-event-name>'.
    #[clap(long, name = "input-formatted-string>:<output-event-name")]
    pub user_event_format_string_name: Vec<RenameMapItem>,

    /// Use a single timeline for all tasks instead of a timeline per task.
    /// ISRs can still be represented with their own timelines or not
    #[clap(long)]
    pub single_task_timeline: bool,

    /// Represent ISR in the parent task context timeline rather than a dedicated ISR timeline
    #[clap(long)]
    pub flatten_isr_timelines: bool,

    /// Use the provided initial startup task name instead of the default ('(startup)')
    #[clap(long, name = "startup-task-name")]
    pub startup_task_name: Option<String>,

    /// Path to memory dump file
    #[clap(value_parser, name = "memory dump file path")]
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
        user_event_channel: opts.user_event_channel,
        user_event_format_string: opts.user_event_format_string,
        user_event_channel_rename_map: opts.user_event_channel_name.into_iter().collect(),
        user_event_format_string_rename_map: opts
            .user_event_format_string_name
            .into_iter()
            .collect(),
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

pub use attr::{AttrKeys, EventAttrKey, TimelineAttrKey};
pub use client::Client;
pub use command::Command;
pub use config::{TraceRecorderConfig, TraceRecorderConfigEntry};
pub use error::Error;
pub use interruptor::Interruptor;
pub use opts::{
    FormatArgAttributeKeysItem, FormatArgAttributeKeysSet, ReflectorOpts, RenameMap, RenameMapItem,
    TraceRecorderOpts,
};
pub use recorder_data::{NanosecondsExt, RecorderDataExt};

pub const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod attr;
pub mod client;
pub mod command;
pub mod config;
pub mod context_manager;
pub mod deviant_event_parser;
pub mod error;
pub mod interruptor;
pub mod opts;
pub mod recorder_data;
pub mod tracing;
pub mod trc_reader;

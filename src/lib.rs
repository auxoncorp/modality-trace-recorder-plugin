#![deny(warnings, clippy::all)]

pub use attr::{AttrKeyIndex, AttrKeys, CommonEventAttrKey, CommonTimelineAttrKey};
pub use auth::AuthTokenBytes;
pub use client::Client;
pub use config::{TraceRecorderConfig, TraceRecorderConfigEntry};
pub use context::{ContextHandle, ContextSwitchOutcome};
pub use import::ImportProtocol;
pub use interruptor::Interruptor;
pub use opts::{
    FormatArgAttributeKeysItem, FormatArgAttributeKeysSet, ReflectorOpts, RenameMap, RenameMapItem,
    TraceRecorderOpts,
};
pub use snapshot::SnapshotFile;
pub use trace_recorder::{NanosecondsExt, TimelineDetails, TraceRecorderExt};

pub mod attr;
pub mod auth;
pub mod client;
pub mod config;
pub mod context;
pub mod import;
pub mod interruptor;
pub mod opts;
pub mod snapshot;
pub mod streaming;
pub mod trace_recorder;
pub mod tracing;

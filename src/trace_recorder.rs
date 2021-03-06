use crate::{AttrKeyIndex, Client, CommonOpts, ContextHandle, RenameMap};
use async_trait::async_trait;
use modality_ingest_client::{
    types::{AttrKey, AttrVal, Nanoseconds},
    IngestError,
};
use std::collections::HashMap;
use thiserror::Error;
use trace_recorder_parser::{
    snapshot, streaming,
    time::{Frequency, Timestamp},
    types::{ObjectHandle, STARTUP_TASK_NAME},
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Missing startup task ('{}') object properties", STARTUP_TASK_NAME)]
    MissingStartupTaskProperties,

    #[error("Failed to locate task properties for object handle {0}")]
    TaskPropertiesLookup(ObjectHandle),

    #[error("Failed to locate ISR properties for object handle {0}")]
    IsrPropertiesLookup(ObjectHandle),

    #[error(transparent)]
    SnapshotTraceRecorder(#[from] snapshot::Error),

    #[error(transparent)]
    StreamingTraceRecorder(#[from] streaming::Error),

    #[error("Encountered an ingest client error. {0}")]
    Ingest(#[from] IngestError),
}

#[derive(Clone, Debug)]
pub struct TraceRecorderConfig {
    pub common: CommonOpts,
    pub user_event_channel: bool,
    pub user_event_format_string: bool,
    pub user_event_channel_rename_map: RenameMap,
    pub user_event_format_string_rename_map: RenameMap,
    pub single_task_timeline: bool,
    pub flatten_isr_timelines: bool,
    pub startup_task_name: Option<String>,
    pub snapshot: bool,
    pub streaming: bool,
}

pub trait NanosecondsExt {
    const ONE_SECOND: u64 = 1_000_000_000;

    fn resolution_ns(&self) -> Option<Nanoseconds>;

    /// Convert to nanosecond time base using the frequency if non-zero,
    /// otherwise fall back to unit ticks
    fn lossy_timestamp_ns(&self, ticks: Timestamp) -> Nanoseconds {
        self.resolution_ns()
            .map(|res| Nanoseconds::from(ticks.get_raw() * res.get_raw()))
            .unwrap_or_else(|| ticks.get_raw().into())
    }
}

impl NanosecondsExt for Frequency {
    fn resolution_ns(&self) -> Option<Nanoseconds> {
        if self.is_unitless() {
            None
        } else {
            Nanoseconds::from(Self::ONE_SECOND / u64::from(self.get_raw())).into()
        }
    }
}

pub struct TimelineDetails<TAK> {
    pub name_key: TAK,
    pub name: String,
    pub description_key: TAK,
    pub description: String,
}

#[async_trait]
pub trait TraceRecorderExt<TAK: AttrKeyIndex, EAK: AttrKeyIndex> {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error>;

    fn timeline_details(
        &self,
        handle: ContextHandle,
        startup_task_name: Option<&str>,
    ) -> Result<TimelineDetails<TAK>, Error>;

    async fn setup_common_timeline_attrs(
        &self,
        cfg: &TraceRecorderConfig,
        client: &mut Client<TAK, EAK>,
    ) -> Result<HashMap<AttrKey, AttrVal>, Error>;
}

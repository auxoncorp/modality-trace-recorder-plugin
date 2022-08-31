use crate::{AttrKeyIndex, Client, ContextHandle, TraceRecorderConfig};
use async_trait::async_trait;
use modality_api::{AttrVal, Nanoseconds};
use modality_ingest_client::IngestError;
use modality_ingest_protocol::InternedAttrKey;
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
    ) -> Result<HashMap<InternedAttrKey, AttrVal>, Error>;
}

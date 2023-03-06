use crate::{
    AttrKeyIndex, Client, ContextHandle, ContextSwitchOutcome, TraceRecorderConfig,
    TraceRecorderExt,
};
use modality_api::{AttrVal, TimelineId};
use modality_ingest_client::{
    IngestClient, IngestClientInitializationError, IngestError, ReadyState,
};
use modality_ingest_protocol::InternedAttrKey;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Seek, SeekFrom},
    str::FromStr,
};
use trace_recorder_parser::{
    streaming::HeaderInfo,
    time::Timestamp,
    types::{
        Argument, FormatString, FormattedString, KernelPortIdentity, ObjectHandle,
        UserEventArgRecordCount, UserEventChannel,
    },
};
use tracing::{debug, error, warn};
use uuid::Uuid;

const TIMELINE_ID_CHANNEL_NAME: &str = "modality-timeline-id";
const TIMELINE_ID_FORMAT_STRING: &str = "name=%s,id=%s";

pub mod snapshot;
pub mod streaming;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kernel port {0} is not supported")]
    UnsupportedKernelPortIdentity(KernelPortIdentity),

    #[error("Failed to locate a timeline for task object handle {0}")]
    TaskTimelineLookup(ObjectHandle),

    #[error("Failed to locate a timeline for ISR object handle {0}")]
    IsrTimelineLookup(ObjectHandle),

    #[error(
        "User events are only allowed to up to {} events",
        UserEventArgRecordCount::MAX
    )]
    ExceededMaxUserEventArgs,

    #[error("The user event format string '{0}' argument count doesn't match the provided custom attribute key set '{1:?}'")]
    FmtArgAttrKeysCountMismatch(String, Vec<String>),

    #[error(transparent)]
    TraceRecorder(#[from] crate::trace_recorder::Error),

    #[error(transparent)]
    SnapshotTraceRecorder(#[from] trace_recorder_parser::snapshot::Error),

    #[error(transparent)]
    StreamingTraceRecorder(#[from] trace_recorder_parser::streaming::Error),

    #[error("Encountered an ingest client initialization error. {0}")]
    IngestClientInitialization(#[from] IngestClientInitializationError),

    #[error("Encountered an ingest client error. {0}")]
    Ingest(#[from] IngestError),

    #[error(transparent)]
    Auth(#[from] crate::auth::AuthTokenError),

    #[error("Encountered an error attemping to parse the protocol parent URL. {0}")]
    Url(#[from] url::ParseError),

    #[error(
        "Encountered and IO error while reading the input stream ({})",
        .0.kind()
    )]
    Io(#[from] io::Error),
}

#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde_with::DeserializeFromStr,
)]
pub enum ImportProtocol {
    Snapshot,
    Streaming,
    Auto,
}

impl Default for ImportProtocol {
    fn default() -> Self {
        ImportProtocol::Auto
    }
}

impl FromStr for ImportProtocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "snapshot" => ImportProtocol::Snapshot,
            "streaming" => ImportProtocol::Streaming,
            "auto" => ImportProtocol::Auto,
            p => return Err(format!("Invalid import protocol '{p}'")),
        })
    }
}

pub async fn import<R: Read + Seek + Send>(
    mut r: R,
    protocol: ImportProtocol,
    cfg: TraceRecorderConfig,
) -> Result<(), Error> {
    match protocol {
        ImportProtocol::Snapshot => snapshot::import(r, cfg).await,
        ImportProtocol::Streaming => streaming::import(r, cfg).await,
        ImportProtocol::Auto => {
            let current_pos = r.stream_position()?;
            debug!("Attempting to detect streaming protocol first");
            let found_psf_word = HeaderInfo::read_psf_word(&mut r).is_ok();
            r.seek(SeekFrom::Start(current_pos))?;
            if found_psf_word {
                streaming::import(r, cfg).await
            } else {
                debug!("Attempting snapshot protocol");
                snapshot::import(r, cfg).await
            }
        }
    }
}

pub(crate) type SnapshotImporter =
    Importer<crate::snapshot::TimelineAttrKey, crate::snapshot::EventAttrKey>;
pub(crate) type StreamingImporter =
    Importer<crate::streaming::TimelineAttrKey, crate::streaming::EventAttrKey>;

pub(crate) struct Importer<TAK: AttrKeyIndex, EAK: AttrKeyIndex> {
    single_task_timeline: bool,
    flatten_isr_timelines: bool,
    use_timeline_id_channel: bool,
    startup_task_name: Option<String>,

    // Used to track interactions between tasks/ISRs
    startup_task_handle: ObjectHandle,
    handle_of_last_logged_context: ContextHandle,
    timestamp_of_last_event: Timestamp,
    object_to_timeline_ids: HashMap<ObjectHandle, TimelineId>,
    registered_object_handles: HashSet<ObjectHandle>,

    common_timeline_attr_kvs: HashMap<InternedAttrKey, AttrVal>,
    client: Client<TAK, EAK>,
}

impl<TAK: AttrKeyIndex, EAK: AttrKeyIndex> Importer<TAK, EAK> {
    async fn begin<TR: TraceRecorderExt<TAK, EAK>>(
        client: IngestClient<ReadyState>,
        cfg: TraceRecorderConfig,
        trd: &TR,
    ) -> Result<Self, Error> {
        let startup_task_handle = trd.startup_task_handle()?;

        // Setup the root startup task timeline
        let startup_task_timeline_id = TimelineId::allocate();
        let mut object_to_timeline_ids = HashMap::new();
        object_to_timeline_ids.insert(startup_task_handle, startup_task_timeline_id);
        let mut registered_object_handles = HashSet::new();
        registered_object_handles.insert(startup_task_handle);
        let mut client = Client::new(client.open_timeline(startup_task_timeline_id).await?);

        let common_timeline_attr_kvs = trd.setup_common_timeline_attrs(&cfg, &mut client).await?;

        let mut importer = Importer {
            single_task_timeline: cfg.plugin.single_task_timeline,
            flatten_isr_timelines: cfg.plugin.flatten_isr_timelines,
            use_timeline_id_channel: cfg.plugin.use_timeline_id_channel,
            startup_task_name: cfg.plugin.startup_task_name,
            startup_task_handle,
            handle_of_last_logged_context: ContextHandle::Task(startup_task_handle),
            timestamp_of_last_event: Timestamp::zero(),
            object_to_timeline_ids,
            registered_object_handles,
            common_timeline_attr_kvs,
            client,
        };

        // Add root startup task timeline metadata
        importer
            .add_timeline_metadata(importer.handle_of_last_logged_context, trd)
            .await?;

        Ok(importer)
    }

    async fn end(self) -> Result<(), Error> {
        self.client.close().await?;
        Ok(())
    }

    fn handle_device_timeline_id_channel_event<TR: TraceRecorderExt<TAK, EAK>>(
        &mut self,
        channel: &UserEventChannel,
        format_string: &FormatString,
        formatted_string: &FormattedString,
        args: &[Argument],
        trd: &TR,
    ) {
        if !self.use_timeline_id_channel || channel.as_str() != TIMELINE_ID_CHANNEL_NAME {
            return;
        }

        if format_string.as_str() != TIMELINE_ID_FORMAT_STRING {
            warn!(
                "Invalid format string '{}' on the channel '{TIMELINE_ID_CHANNEL_NAME}'",
                format_string
            );
            return;
        }

        if args.len() != 2 {
            warn!(
                "Invalid argument length in '{}' on channel '{TIMELINE_ID_CHANNEL_NAME}'",
                formatted_string
            );
            return;
        }

        let obj_name = match &args[0] {
            Argument::String(s) => s.as_str(),
            _ => {
                warn!(
                    "Invalid argument[0] type in '{}' on channel '{TIMELINE_ID_CHANNEL_NAME}'",
                    formatted_string
                );
                return;
            }
        };

        let obj_handle = match trd.object_handle(obj_name) {
            Some(h) => h,
            None => {
                warn!(
                    "Object with name '{obj_name}' has not be registered yet, ignoring timeline-id"
                );
                return;
            }
        };

        if self.object_to_timeline_ids.contains_key(&obj_handle) {
            warn!(
                    "Object with name '{obj_name}' already has a timeline-id, ignoring provided timeline-id"
                );
            return;
        }

        let timeline_id = match &args[1] {
            Argument::String(s) => match Uuid::parse_str(s.as_str()) {
                Ok(id) => TimelineId::from(id),
                Err(e) => {
                    warn!("The provided timeline-id '{s}' is invalid. {e}");
                    return;
                }
            },
            _ => {
                warn!(
                    "Invalid argument[1] type in '{}' on channel '{TIMELINE_ID_CHANNEL_NAME}'",
                    formatted_string
                );
                return;
            }
        };

        self.object_to_timeline_ids.insert(obj_handle, timeline_id);
    }

    async fn event_key<K: Into<EAK>>(&mut self, key: K) -> Result<InternedAttrKey, Error> {
        let k = self.client.event_key(key).await?;
        Ok(k)
    }

    async fn event(
        &mut self,
        timestamp: Timestamp,
        ordering: u128,
        attrs: impl IntoIterator<Item = (InternedAttrKey, AttrVal)>,
    ) -> Result<(), Error> {
        if timestamp < self.timestamp_of_last_event {
            warn!(
                "Time went backwards from {} to {}",
                self.timestamp_of_last_event, timestamp
            );
        }

        // Keep track of this for interaction remote timestamp on next task switch
        self.timestamp_of_last_event = timestamp;

        self.client.inner().event(ordering, attrs).await?;

        Ok(())
    }

    /// Called on the task or ISR context switch-in events
    ///
    /// EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
    /// EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
    async fn context_switch_in<TR: TraceRecorderExt<TAK, EAK>>(
        &mut self,
        event_context: ContextHandle,
        trd: &TR,
    ) -> Result<ContextSwitchOutcome, Error> {
        // We've resumed from the same context we were already in and not doing
        // any flattening, nothing to do
        if !self.single_task_timeline
            && !self.flatten_isr_timelines
            && (event_context == self.handle_of_last_logged_context)
        {
            return Ok(ContextSwitchOutcome::Same);
        }

        // * a task or ISR context in normal operation
        // * a task context (!single_task_timeline && flatten_isr_timelines)
        // * startup task context or ISR context (single_task_timeline && !flatten_isr_timelines)
        // * always startup task context (single_task_timeline && flatten_isr_timelines)
        let (prev_handle, prev_timeline_id) = match self.handle_of_last_logged_context {
            ContextHandle::Task(h) => (
                ContextHandle::Task(h),
                *self
                    .object_to_timeline_ids
                    .get(&h)
                    .ok_or(Error::TaskTimelineLookup(h))?,
            ),
            ContextHandle::Isr(h) => (
                ContextHandle::Isr(h),
                *self
                    .object_to_timeline_ids
                    .get(&h)
                    .ok_or(Error::IsrTimelineLookup(h))?,
            ),
        };

        let mut timeline_is_new = false;
        let curr_timeline_id = match event_context {
            // EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
            // Resume could be same task or from an ISR, when the task is already marked as active
            ContextHandle::Task(task_event_handle) => {
                let handle = if self.single_task_timeline {
                    self.startup_task_handle
                } else {
                    task_event_handle
                };
                self.handle_of_last_logged_context = ContextHandle::Task(handle);
                timeline_is_new = self.registered_object_handles.insert(handle);
                *self
                    .object_to_timeline_ids
                    .entry(handle)
                    .or_insert_with(TimelineId::allocate)
            }
            // EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
            // Resume happens when we return to another ISR
            ContextHandle::Isr(isr_event_handle) => {
                if self.flatten_isr_timelines {
                    // Flatten the ISR context into the parent task context
                    self.handle_of_last_logged_context = prev_handle;
                    prev_timeline_id
                } else {
                    self.handle_of_last_logged_context = ContextHandle::Isr(isr_event_handle);
                    timeline_is_new = self.registered_object_handles.insert(isr_event_handle);
                    *self
                        .object_to_timeline_ids
                        .entry(isr_event_handle)
                        .or_insert_with(TimelineId::allocate)
                }
            }
        };

        self.client.inner().open_timeline(curr_timeline_id).await?;
        if timeline_is_new {
            // Add timeline metadata in the newly updated context
            self.add_timeline_metadata(self.handle_of_last_logged_context, trd)
                .await?;
        }

        if prev_timeline_id != curr_timeline_id {
            Ok(ContextSwitchOutcome::Different(
                prev_timeline_id,
                self.timestamp_of_last_event,
            ))
        } else {
            Ok(ContextSwitchOutcome::Same)
        }
    }

    async fn add_timeline_metadata<TR: TraceRecorderExt<TAK, EAK>>(
        &mut self,
        handle: ContextHandle,
        trd: &TR,
    ) -> Result<(), Error> {
        let tl_details = trd.timeline_details(handle, self.startup_task_name.as_deref())?;

        let mut attr_kvs = self.common_timeline_attr_kvs.clone();
        attr_kvs.insert(
            self.client.timeline_key(tl_details.name_key).await?,
            tl_details.name.into(),
        );
        attr_kvs.insert(
            self.client.timeline_key(tl_details.description_key).await?,
            tl_details.description.into(),
        );
        attr_kvs.insert(
            self.client
                .timeline_key(tl_details.object_handle_key)
                .await?,
            i64::from(u32::from(tl_details.object_handle)).into(),
        );

        self.client.inner().timeline_metadata(attr_kvs).await?;

        Ok(())
    }
}

pub(crate) fn arg_to_attr_val(arg: &Argument) -> AttrVal {
    match arg {
        Argument::I8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::F32(v) => AttrVal::from(f64::from(v.0)),
        Argument::F64(v) => AttrVal::from(v.0),
        Argument::String(v) => AttrVal::String(v.clone()),
    }
}

use crate::{
    attr::EventAttrKey,
    client::Client,
    config::TraceRecorderConfig,
    error::Error,
    recorder_data::{ContextHandle, RecorderDataExt},
};
use auxon_sdk::{
    api::{AttrVal, TimelineId},
    ingest_client::{IngestClient, ReadyState},
    ingest_protocol::InternedAttrKey,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use trace_recorder_parser::{
    time::Timestamp,
    types::{Argument, FormatString, FormattedString, ObjectHandle, UserEventChannel},
};
use tracing::warn;
use uuid::Uuid;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum ContextSwitchOutcome {
    /// Current task/ISR is already marked as active, we're on the same timeline
    Same,
    /// Switched to a new task/ISR, return the remote (previous) timeline ID and
    /// the last event timestamped logged on the previous timeline
    Different(TimelineId, Timestamp),
}

const TIMELINE_ID_CHANNEL_NAME: &str = "modality_timeline_id";
const TIMELINE_ID_FORMAT_STRING: &str = "name=%s,id=%s";

pub(crate) struct ContextManager {
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
    client: Client,
}

impl ContextManager {
    pub(crate) async fn begin<TR: RecorderDataExt>(
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

        //let common_timeline_attr_kvs = trd.setup_common_timeline_attrs(&cfg, &mut client).await?;
        let common_timeline_attr_kvs = Default::default();

        let mut importer = ContextManager {
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

    pub(crate) async fn end(self) -> Result<(), Error> {
        self.client.close().await?;
        Ok(())
    }

    pub(crate) fn set_degraded_single_timeline_mode(&mut self) {
        self.single_task_timeline = true;
        self.flatten_isr_timelines = true;
        self.handle_of_last_logged_context = ContextHandle::Task(self.startup_task_handle);
    }

    pub(crate) fn handle_device_timeline_id_channel_event<TR: RecorderDataExt>(
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

    pub(crate) async fn event_key(&mut self, key: EventAttrKey) -> Result<InternedAttrKey, Error> {
        let k = self.client.event_key(key).await?;
        Ok(k)
    }

    pub(crate) async fn event(
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
    pub(crate) async fn context_switch_in<TR: RecorderDataExt>(
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

    pub(crate) async fn add_timeline_metadata<TR: RecorderDataExt>(
        &mut self,
        handle: ContextHandle,
        trd: &TR,
    ) -> Result<(), Error> {
        /*
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
        */

        Ok(())
    }
}

/*
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
        Argument::String(v) => AttrVal::String(v.clone().into()),
    }
}
*/

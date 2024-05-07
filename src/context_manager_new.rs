use crate::{
    attr::{EventAttrKey, TimelineAttrKey},
    config::{PluginConfig, TraceRecorderConfig},
    error::Error,
    recorder_data::{ContextHandle, EventAttributes, RecorderDataExt, TimelineAttributes},
};
use auxon_sdk::api::{AttrVal, TimelineId};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use trace_recorder_parser::{
    streaming::event::{Event, EventCode, EventType, TrackingEventCounter},
    time::{StreamingInstant, Timestamp},
    types::{Argument, FormatString, FormattedString, ObjectHandle, UserEventChannel},
};
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Debug)]
pub struct ContextEvent {
    pub context: ObjectHandle,
    pub global_ordering: u128,
    pub attributes: EventAttributes,
    // In fully-linearized interaction mode, every event has a nonce.
    // When true, this event contains an interaction from the previous
    // event and the previous event's nonce should be visible in
    // the conventional attribute key.
    pub add_previous_event_nonce: bool,
}

type RemoteTimelineId = TimelineId;
type RemoteInteractionNonce = i64;
type InteractionNonce = i64;
type RemoteContext = ObjectHandle;
type ContextSwitchInteraction = (RemoteContext, RemoteTimelineId, RemoteInteractionNonce);

#[derive(Debug)]
pub struct TimelineMeta {
    id: TimelineId,
    context: ObjectHandle,
    attributes: TimelineAttributes,
    /// The nonce recorded on the last event.
    /// Effectively a timeline-local event counter so we can draw arbitrary interactions
    nonce: InteractionNonce,
}

#[derive(Debug)]
pub struct ContextManager {
    cfg: TraceRecorderConfig,
    common_timeline_attrs: TimelineAttributes,

    global_ordering: u128,
    time_rollover_tracker: StreamingInstant,
    event_counter_tracker: TrackingEventCounter,

    objects_to_timelines: HashMap<ObjectHandle, TimelineMeta>,
    // State for fully-linearized interaction mode
    // TODO
    // State for ipc interaction mode
    // TODO
}

impl ContextManager {
    pub fn new<TR: RecorderDataExt>(cfg: TraceRecorderConfig, trd: &TR) -> Result<Self, Error> {
        todo!()
    }

    pub fn timeline_meta(&self, handle: ObjectHandle) -> Result<&TimelineMeta, Error> {
        self.objects_to_timelines
            .get(&handle)
            .ok_or(Error::TaskTimelineLookup(handle))
    }

    pub fn process_event<TR: RecorderDataExt>(
        &mut self,
        event_code: EventCode,
        event: &Event,
        trd: &TR,
    ) -> Result<ContextEvent, Error> {
        // NOTE: We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        self.global_ordering = self.global_ordering.saturating_add(1);

        todo!()
    }
}

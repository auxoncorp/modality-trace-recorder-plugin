use crate::{
    attr::{EventAttrKey, TimelineAttrKey},
    config::TraceRecorderConfig,
    deviant_event_parser::DeviantEventParser,
    error::Error,
    opts::InteractionMode,
    recorder_data::{
        ContextHandle, EventAttributes, NanosecondsExt, RecorderDataExt, TimelineAttributes,
    },
};
use auxon_sdk::api::{Nanoseconds, TimelineId};
use std::collections::{HashMap, VecDeque};
use trace_recorder_parser::{
    streaming::event::{Event, EventCode, EventType, TrackingEventCounter},
    streaming::RecorderData,
    time::{StreamingInstant, Timestamp},
    types::{ObjectClass, ObjectHandle},
};
use tracing::{debug, trace, warn};

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

#[derive(Debug)]
pub struct TimelineMeta {
    id: TimelineId,
    attributes: TimelineAttributes,
    /// The nonce recorded on the last event.
    /// Effectively a timeline-local event counter so we can draw arbitrary interactions
    nonce: InteractionNonce,
}

#[derive(Debug)]
pub struct ContextManager {
    cfg: TraceRecorderConfig,
    common_timeline_attributes: TimelineAttributes,

    global_ordering: u128,
    time_rollover_tracker: StreamingInstant,
    event_counter_tracker: TrackingEventCounter,

    degraded: bool,
    first_event_observed: bool,
    deviant_event_parser: Option<DeviantEventParser>,
    objects_to_timelines: HashMap<ObjectHandle, TimelineMeta>,
    startup_task_handle: ObjectHandle,

    // State for interaction modes
    active_context: ContextHandle,
    notif_pending_interactions: HashMap<DestinationContextHandle, Interaction>,
    queue_pending_interactions: HashMap<QueueHandle, VecDeque<Interaction>>,

    // State for stats
    utilization_measurement_window_ticks: u64,
    last_running_timestamp: Timestamp,
    utilization_measurement_window_start: Timestamp,
    context_stats: HashMap<ContextHandle, ContextStats>,
}

impl ContextManager {
    const UNKNOWN_CONTEXT: &'static str = "UNKNOWN_CONTEXT";
    const DEFAULT_MEASURMENT_WINDOW_MS: u64 = 500;
    const DEFAULT_MEASURMENT_WINDOW_TICKS: u64 = 1_000_000; // from a hat

    pub fn new(mut cfg: TraceRecorderConfig, trd: &RecorderData) -> Result<Self, Error> {
        if cfg.plugin.interaction_mode != InteractionMode::FullyLinearized {
            if cfg.plugin.single_task_timeline {
                warn!(interaction_mode = ?cfg.plugin.interaction_mode,
                    "Configuration 'single-task-timeline' is ignored by the current interaction mode");
                cfg.plugin.single_task_timeline = false;
            }
            if cfg.plugin.flatten_isr_timelines {
                warn!(interaction_mode = ?cfg.plugin.interaction_mode,
                    "Configuration 'flatten-isr-timelines' is ignored by the current interaction mode");
                cfg.plugin.flatten_isr_timelines = false;
            }
        }

        let utilization_measurement_window_ms =
            if let Some(cfg_ns) = cfg.plugin.cpu_utilization_measurement_window.as_ref() {
                // min == 1ms
                std::cmp::max(cfg_ns.0.as_millis() as u64, 1)
            } else {
                Self::DEFAULT_MEASURMENT_WINDOW_MS
            };
        let utilization_measurement_window_ticks = if trd
            .timestamp_info
            .timer_frequency
            .is_unitless()
        {
            warn!(
                ticks = Self::DEFAULT_MEASURMENT_WINDOW_TICKS,
                "Target didn't report the timestamp frequency, CPU utilization will be measuremnt over default ticks");
            Self::DEFAULT_MEASURMENT_WINDOW_TICKS
        } else {
            let ticks_per_ms = u64::from(trd.timestamp_info.timer_frequency.get_raw()) / 1_000;
            ticks_per_ms * utilization_measurement_window_ms
        };
        let utilization_measurement_window_ns = utilization_measurement_window_ms * 1_000_000;

        let startup_task_handle = trd.startup_task_handle()?;

        let deviant_event_parser = if let Some(base_event_id) = cfg.plugin.deviant_event_id_base {
            Some(DeviantEventParser::new(
                trd.header.endianness,
                base_event_id.into(),
            )?)
        } else {
            None
        };

        let mut common_timeline_attributes = trd.common_timeline_attributes(&cfg);
        common_timeline_attributes.insert(
            TimelineAttrKey::CpuUtilizationMeasurementWindowTicks,
            utilization_measurement_window_ticks.into(),
        );
        common_timeline_attributes.insert(
            TimelineAttrKey::CpuUtilizationMeasurementWindow,
            Nanoseconds::from(utilization_measurement_window_ns).into(),
        );

        Ok(Self {
            cfg,
            common_timeline_attributes,
            global_ordering: 0,
            // NOTE: timestamp/event trackers get re-initialized on the first event
            time_rollover_tracker: StreamingInstant::zero(),
            event_counter_tracker: TrackingEventCounter::zero(),
            degraded: false,
            first_event_observed: false,
            deviant_event_parser,
            objects_to_timelines: Default::default(),
            startup_task_handle,
            active_context: ContextHandle::Task(startup_task_handle),
            notif_pending_interactions: Default::default(),
            queue_pending_interactions: Default::default(),
            utilization_measurement_window_ticks,
            last_running_timestamp: Timestamp::zero(),
            utilization_measurement_window_start: Timestamp::zero(),
            context_stats: Default::default(),
        })
    }

    pub fn timeline_meta(&self, handle: ObjectHandle) -> Result<&TimelineMeta, Error> {
        self.objects_to_timelines
            .get(&handle)
            .ok_or(Error::TaskTimelineLookup(handle))
    }

    pub fn observe_trace_restart(&mut self) {
        self.first_event_observed = false;
        self.context_stats.clear();
    }

    pub fn set_degraded<S: ToString>(&mut self, reason: S) {
        if !self.degraded {
            warn!(
                reason = reason.to_string(),
                "Downgrading to single timeline mode"
            );
            self.degraded = true;
            self.cfg.plugin.interaction_mode = InteractionMode::FullyLinearized;
            self.cfg.plugin.single_task_timeline = true;
            self.cfg.plugin.flatten_isr_timelines = true;
            self.cfg.plugin.startup_task_name = Some(Self::UNKNOWN_CONTEXT.to_owned());
            self.active_context = ContextHandle::Task(self.startup_task_handle);
        }
    }

    pub fn process_event(
        &mut self,
        event_code: EventCode,
        event: &Event,
        trd: &RecorderData,
    ) -> Result<Option<ContextEvent>, Error> {
        // NOTE: We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        self.global_ordering = self.global_ordering.saturating_add(1);

        let event_type = event_code.event_type();

        let dropped_events = if !self.first_event_observed {
            if event_type != EventType::TraceStart {
                self.set_degraded(format!(
                    "First event should be TRACE_START (got {})",
                    event_type
                ));
            }

            self.event_counter_tracker
                .set_initial_count(event.event_count());

            self.time_rollover_tracker = StreamingInstant::new(
                event.timestamp().ticks() as u32,
                trd.timestamp_info.timer_wraparounds,
            );

            self.utilization_measurement_window_start = self.time_rollover_tracker.to_timestamp();
            for s in self.context_stats.values_mut() {
                s.reset_runtime_since_window_start();
            }

            // Setup initial root/startup task
            if let Event::TraceStart(ev) = event {
                let class = trd
                    .entry_table
                    .class(ev.current_task_handle)
                    .unwrap_or(ObjectClass::Task);
                if class == ObjectClass::Task {
                    self.active_context = ContextHandle::Task(ev.current_task_handle);
                } else {
                    self.active_context = ContextHandle::Isr(ev.current_task_handle);
                }
            } else {
                self.active_context = ContextHandle::Task(self.startup_task_handle);
            }
            self.alloc_context(self.active_context, trd)?;
            let _ctx_stats = self
                .context_stats
                .entry(self.active_context)
                .or_insert_with(|| ContextStats::new(self.time_rollover_tracker.to_timestamp()));

            self.first_event_observed = true;
            None
        } else {
            self.event_counter_tracker.update(event.event_count())
        };

        // Update timer/counter rollover trackers
        let event_count_raw = u16::from(event.event_count());
        let event_count = self.event_counter_tracker.count();
        let timer_ticks = event.timestamp();
        let timestamp = self.time_rollover_tracker.elapsed(timer_ticks);
        self.last_running_timestamp = timestamp;

        let mut attrs: EventAttributes = Default::default();

        // Skip events that are not relevant or are not expected
        match event {
            Event::ObjectName(_) | Event::TsConfig(_) => return Ok(None),
            Event::Unknown(ev) => {
                // Handle custom deviant events
                let maybe_dev = if let Some(p) = self.deviant_event_parser.as_mut() {
                    p.parse(ev)?
                } else {
                    None
                };

                if let Some(dev) = maybe_dev {
                    attrs.insert(EventAttrKey::Name, dev.kind.to_modality_name().into());
                    attrs.insert(EventAttrKey::MutatorId, dev.mutator_id.1);
                    attrs.insert(
                        EventAttrKey::InternalMutatorId,
                        dev.mutator_id.0.to_string().into(),
                    );
                    if let Some(m) = dev.mutation_id {
                        attrs.insert(EventAttrKey::MutationId, m.1);
                        attrs.insert(EventAttrKey::InternalMutationId, m.0.to_string().into());
                    }
                    if let Some(s) = dev.mutation_success {
                        attrs.insert(EventAttrKey::MutationSuccess, s);
                    }
                } else if !self.cfg.plugin.include_unknown_events {
                    debug!(
                          %event_type,
                          timestamp = %ev.timestamp,
                          id = %ev.code.event_id(),
                          event_count = %ev.event_count, "Skipping unknown");
                    return Ok(None);
                }
            }
            _ => (),
        }

        // Add core attr set
        trd.add_core_event_attributes(&self.cfg.plugin, event_code, event, &mut attrs)?;

        // Add event counter attrs
        attrs.insert(EventAttrKey::EventCountRaw, event_count_raw.into());
        attrs.insert(EventAttrKey::EventCount, event_count.into());
        if let Some(dropped_events) = dropped_events {
            warn!(
                event_count = event_count_raw,
                dropped_events, "Dropped events detected"
            );

            attrs.insert(EventAttrKey::DroppedEvents, dropped_events.into());
        }

        // Add timerstamp attrs
        attrs.insert(EventAttrKey::TimerTicks, timer_ticks.ticks().into());
        attrs.insert(EventAttrKey::TimestampTicks, timestamp.ticks().into());
        if let Some(ns) = trd
            .timestamp_info
            .timer_frequency
            .convert_timestamp(timestamp)
        {
            attrs.insert(EventAttrKey::Timestamp, ns.into());
        }

        let ctx_switch_outcome = self.handle_context_switch(event, trd)?;

        let mut ctx_event = ContextEvent {
            context: self.active_context.object_handle(),
            global_ordering: self.global_ordering,
            attributes: attrs,
            add_previous_event_nonce: false,
        };

        // Add stats attributes when we get a context switch event
        if matches!(ctx_switch_outcome, ContextSwitchOutcome::Different(_)) {
            let diff = self.last_running_timestamp - self.utilization_measurement_window_start;
            if diff.ticks() > self.utilization_measurement_window_ticks {
                for s in self.context_stats.values_mut() {
                    s.update_runtime_window(diff);
                }
                self.utilization_measurement_window_start = self.last_running_timestamp;
            }

            if let Some(ctx_stats) = self.context_stats.get(&self.active_context) {
                ctx_event.add_stats(trd, self.last_running_timestamp, ctx_stats);
            }
        }

        // Handle interaction mode specifics
        match self.cfg.plugin.interaction_mode {
            InteractionMode::FullyLinearized => Ok(Some(
                self.process_fully_linearized_mode(ctx_switch_outcome, ctx_event)?,
            )),
            InteractionMode::Ipc => Ok(Some(self.process_ipc_mode(event, ctx_event)?)),
        }
    }

    fn process_fully_linearized_mode(
        &mut self,
        ctx_switch_outcome: ContextSwitchOutcome,
        mut ctx_event: ContextEvent,
    ) -> Result<ContextEvent, Error> {
        match ctx_switch_outcome {
            ContextSwitchOutcome::Same => {
                // Normal event on the active context
            }
            ContextSwitchOutcome::Different(prev_context) => {
                // Context switch event
                ctx_event.add_previous_event_nonce = true;
                let prev_timeline = self.timeline_mut(prev_context)?;
                let interaction_src = prev_timeline.interaction_source();
                ctx_event.add_interaction(interaction_src);
            }
        }

        let timeline = self.timeline_mut(self.active_context)?;
        timeline.increment_nonce();
        ctx_event.add_internal_nonce(timeline.nonce);

        Ok(ctx_event)
    }

    fn process_ipc_mode(
        &mut self,
        event: &Event,
        mut ctx_event: ContextEvent,
    ) -> Result<ContextEvent, Error> {
        let timeline = self.timeline_mut(self.active_context)?;
        timeline.increment_nonce();
        ctx_event.add_internal_nonce(timeline.nonce);

        match event {
            Event::TaskNotify(ev) | Event::TaskNotifyFromIsr(ev) => {
                // Record the interaction source (sender task)
                ctx_event.promote_internal_nonce();
                let handle_of_task_to_notify = ev.handle;
                let interaction_src = timeline.interaction_source();
                // NOTE: for task notifications, only the immediately preceding sender (most recent)
                // is captured
                let _maybe_overwritten_pending_interaction = self
                    .notif_pending_interactions
                    .insert(handle_of_task_to_notify, interaction_src);
            }
            Event::TaskNotifyWait(ev) => {
                // Add any pending remote interaction (recvr task)
                if ev.handle != self.active_context.object_handle() {
                    warn!(
                        notif_wait_handle = %ev.handle,
                        active_context = %self.active_context.object_handle(),
                        "Inconsistent IPC interaction context");
                }
                if let Some(pending_interaction) =
                    self.notif_pending_interactions.remove(&ev.handle)
                {
                    ctx_event.add_interaction(pending_interaction);
                }
            }

            Event::QueueSend(ev) | Event::QueueSendFromIsr(ev) => {
                // Record the interaction source (sender task), send to back
                ctx_event.promote_internal_nonce();
                let interaction_src = timeline.interaction_source();
                let pending_interactions = self
                    .queue_pending_interactions
                    .entry(ev.handle)
                    .or_default();
                pending_interactions.push_back(interaction_src);
            }
            Event::QueueSendFront(ev) | Event::QueueSendFrontFromIsr(ev) => {
                // Record the interaction source (sender task), send to front
                ctx_event.promote_internal_nonce();
                let interaction_src = timeline.interaction_source();
                let pending_interactions = self
                    .queue_pending_interactions
                    .entry(ev.handle)
                    .or_default();
                pending_interactions.push_front(interaction_src);
            }
            Event::QueueReceive(ev) | Event::QueueReceiveFromIsr(ev) => {
                // Add any pending remote interaction (recvr task)
                let pending_interactions = self
                    .queue_pending_interactions
                    .entry(ev.handle)
                    .or_default();
                if let Some(pending_interaction) = pending_interactions.pop_front() {
                    ctx_event.add_interaction(pending_interaction);
                }
            }
            Event::QueuePeek(ev) => {
                // Add any pending remote interaction (recvr task), but don't remove it
                let pending_interactions = self
                    .queue_pending_interactions
                    .entry(ev.handle)
                    .or_default();
                if let Some(pending_interaction) = pending_interactions.pop_front() {
                    pending_interactions.push_front(pending_interaction);
                    ctx_event.add_interaction(pending_interaction);
                }
            }

            _ => (),
        }

        Ok(ctx_event)
    }

    fn handle_context_switch(
        &mut self,
        event: &Event,
        trd: &RecorderData,
    ) -> Result<ContextSwitchOutcome, Error> {
        let maybe_contex_switch_handle: Option<ContextHandle> = match event {
            Event::IsrBegin(ev) | Event::IsrResume(ev) => Some(ev.into()),
            Event::TaskBegin(ev) | Event::TaskResume(ev) | Event::TaskActivate(ev) => {
                Some(ev.into())
            }
            _ => None,
        };

        match maybe_contex_switch_handle {
            Some(context) => self.context_switch_in(context, trd),
            None => Ok(ContextSwitchOutcome::Same),
        }
    }

    /// Called on the task or ISR context switch-in events
    ///
    /// EventType:
    /// * TaskSwitchTaskBegin
    /// * TaskSwitchTaskResume
    /// * TaskActivate
    /// * TaskSwitchIsrBegin
    /// * TaskSwitchIsrResume
    fn context_switch_in(
        &mut self,
        context: ContextHandle,
        trd: &RecorderData,
    ) -> Result<ContextSwitchOutcome, Error> {
        if context != self.active_context {
            // Update runtime stats for the previous context being switched out
            if let Some(prev_ctx_stats) = self.context_stats.get_mut(&self.active_context) {
                prev_ctx_stats.update(self.last_running_timestamp);
            }

            // Same for the new context being switched in
            let ctx_stats = self
                .context_stats
                .entry(context)
                .or_insert_with(|| ContextStats::new(self.last_running_timestamp));
            ctx_stats.set_last_timestamp(self.last_running_timestamp);
        }

        // We've resumed from the same context we were already in and not doing
        // any flattening, nothing left to do
        if !self.cfg.plugin.single_task_timeline
            && !self.cfg.plugin.flatten_isr_timelines
            && (context == self.active_context)
        {
            return Ok(ContextSwitchOutcome::Same);
        }

        // self.active_context:
        // * a task or ISR context in normal operation
        // * a task context (!single_task_timeline && flatten_isr_timelines)
        // * startup task context or ISR context (single_task_timeline && !flatten_isr_timelines)
        // * always startup task context (single_task_timeline && flatten_isr_timelines)

        let new_context = match context {
            // EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
            // Resume could be same task or from an ISR, when the task is already marked as active
            ContextHandle::Task(task_event_handle) => {
                let handle = if self.cfg.plugin.single_task_timeline {
                    self.startup_task_handle
                } else {
                    task_event_handle
                };
                ContextHandle::Task(handle)
            }
            // EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
            // Resume happens when we return to another ISR
            ContextHandle::Isr(isr_event_handle) => {
                if self.cfg.plugin.flatten_isr_timelines {
                    // Flatten the ISR context into the parent task context
                    self.active_context
                } else {
                    ContextHandle::Isr(isr_event_handle)
                }
            }
        };

        if new_context != self.active_context {
            self.alloc_context(new_context, trd)?;
            let prev_context = self.active_context;
            self.active_context = new_context;
            Ok(ContextSwitchOutcome::Different(prev_context))
        } else {
            Ok(ContextSwitchOutcome::Same)
        }
    }

    fn timeline_mut(&mut self, context: ContextHandle) -> Result<&mut TimelineMeta, Error> {
        match context {
            ContextHandle::Task(h) => self
                .objects_to_timelines
                .get_mut(&h)
                .ok_or(Error::TaskTimelineLookup(h)),
            ContextHandle::Isr(h) => self
                .objects_to_timelines
                .get_mut(&h)
                .ok_or(Error::IsrTimelineLookup(h)),
        }
    }

    fn alloc_context(&mut self, context: ContextHandle, trd: &RecorderData) -> Result<(), Error> {
        let mut is_new = false;
        let tlm = self
            .objects_to_timelines
            .entry(context.object_handle())
            .or_insert_with(|| {
                is_new = true;
                let attrs = self.common_timeline_attributes.clone();
                TimelineMeta::new(context.object_handle(), attrs)
            });
        if is_new {
            trd.add_core_timeline_attributes(&self.cfg.plugin, context, &mut tlm.attributes)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
enum ContextSwitchOutcome {
    /// Current task/ISR is already marked as active, we're on the same timeline
    Same,
    /// Switched to a new task/ISR, return the previous context
    Different(ContextHandle),
}

type DurationTicks = Timestamp;

type QueueHandle = ObjectHandle;
type DestinationContextHandle = ObjectHandle;

type RemoteTimelineId = TimelineId;
type RemoteInteractionNonce = i64;
type InteractionNonce = i64;
type Interaction = (RemoteTimelineId, RemoteInteractionNonce);

impl ContextEvent {
    pub fn promote_internal_nonce(&mut self) {
        if let Some(nonce) = self.attributes.remove(&EventAttrKey::InternalNonce) {
            self.attributes.insert(EventAttrKey::Nonce, nonce);
        }
    }

    fn add_internal_nonce(&mut self, nonce: InteractionNonce) {
        self.attributes
            .insert(EventAttrKey::InternalNonce, nonce.into());
    }

    fn add_interaction(&mut self, interaction: Interaction) {
        self.attributes
            .insert(EventAttrKey::RemoteTimelineId, interaction.0.into());
        self.attributes
            .insert(EventAttrKey::RemoteNonce, interaction.1.into());
    }

    fn add_stats(
        &mut self,
        trd: &RecorderData,
        total_runtime: DurationTicks,
        ctx_stats: &ContextStats,
    ) {
        self.attributes.insert(
            EventAttrKey::TotalRuntimeTicks,
            total_runtime.ticks().into(),
        );
        if let Some(ns) = trd
            .timestamp_info
            .timer_frequency
            .convert_timestamp(total_runtime)
        {
            self.attributes
                .insert(EventAttrKey::TotalRuntime, ns.into());
        }
        self.attributes.insert(
            EventAttrKey::RuntimeTicks,
            ctx_stats.total_runtime.ticks().into(),
        );
        if let Some(ns) = trd
            .timestamp_info
            .timer_frequency
            .convert_timestamp(ctx_stats.total_runtime)
        {
            self.attributes.insert(EventAttrKey::Runtime, ns.into());
        }

        if let Some(win) = ctx_stats.last_runtime_window.as_ref() {
            let ctx_runtime = win.0;
            let window_dur = win.1;

            self.attributes.insert(
                EventAttrKey::RuntimeInWindowTicks,
                ctx_runtime.ticks().into(),
            );
            if let Some(ns) = trd
                .timestamp_info
                .timer_frequency
                .convert_timestamp(ctx_runtime)
            {
                self.attributes
                    .insert(EventAttrKey::RuntimeInWindow, ns.into());
            }

            self.attributes
                .insert(EventAttrKey::RuntimeWindowTicks, window_dur.ticks().into());
            if let Some(ns) = trd
                .timestamp_info
                .timer_frequency
                .convert_timestamp(window_dur)
            {
                self.attributes
                    .insert(EventAttrKey::RuntimeWindow, ns.into());
            }

            if window_dur.ticks() > 0 {
                let runtime_utilization = ctx_runtime.ticks() as f64 / window_dur.ticks() as f64;

                self.attributes
                    .insert(EventAttrKey::CpuUtilization, runtime_utilization.into());
            }
        }
    }
}

impl TimelineMeta {
    fn new(context: ObjectHandle, attributes: TimelineAttributes) -> Self {
        let id = TimelineId::allocate();
        trace!(%context, timeline_id = %id, "Creating timeline metadata");
        Self {
            id,
            attributes,
            nonce: 0,
        }
    }

    fn increment_nonce(&mut self) {
        self.nonce = self.nonce.wrapping_add(1);
    }

    fn interaction_source(&self) -> Interaction {
        (self.id, self.nonce)
    }

    pub fn id(&self) -> TimelineId {
        self.id
    }

    pub fn attributes(&self) -> &TimelineAttributes {
        &self.attributes
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
struct ContextStats {
    /// When the context was last switched in
    last_timestamp: Timestamp,

    /// Total time the context has been in the running state
    total_runtime: DurationTicks,

    /// Time the context has been in the running state since
    /// the window start
    runtime_since_window_start: ContextRunningDuration,

    /// Previously observed window state: time this context
    /// was in the running state and total duration
    /// of the window
    last_runtime_window: Option<(ContextRunningDuration, RuntimeWindowDuration)>,
}

type RuntimeWindowDuration = DurationTicks;
type ContextRunningDuration = DurationTicks;

impl ContextStats {
    fn new(last_timestamp: Timestamp) -> Self {
        Self {
            last_timestamp,
            total_runtime: DurationTicks::zero(),
            runtime_since_window_start: DurationTicks::zero(),
            last_runtime_window: None,
        }
    }

    /// Called when runtime window is reset
    fn reset_runtime_since_window_start(&mut self) {
        self.runtime_since_window_start = DurationTicks::zero();
        self.last_runtime_window = None;
    }

    /// Called when a new window interval starts
    fn update_runtime_window(&mut self, runtime_dur: RuntimeWindowDuration) {
        self.last_runtime_window = Some((self.runtime_since_window_start, runtime_dur));
        self.runtime_since_window_start = DurationTicks::zero();
    }

    /// Called when this context is switched in
    fn set_last_timestamp(&mut self, last_timestamp: Timestamp) {
        self.last_timestamp = last_timestamp;
    }

    /// Called when this context is switched out
    fn update(&mut self, timestamp: Timestamp) {
        if timestamp < self.last_timestamp {
            warn!("Stats timestamp went backwards");
        } else {
            let diff = timestamp - self.last_timestamp;
            self.total_runtime += diff;
            self.runtime_since_window_start += diff;
            self.last_timestamp = timestamp;
        }
    }
}

use derive_more::Display;
use modality_ingest_client::{types::AttrKey, BoundTimelineState, IngestClient, IngestError};
use std::{collections::HashMap, hash::Hash};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum TimelineAttrKey {
    #[display(fmt = "timeline.name")]
    Name,
    #[display(fmt = "timeline.id")]
    Id,
    #[display(fmt = "timeline.description")]
    Description,
    #[display(fmt = "timeline.run_id")]
    RunId,
    #[display(fmt = "timeline.time_domain")]
    TimeDomain,
    #[display(fmt = "timeline.time_resolution")]
    TimeResolution,

    #[display(fmt = "timeline.internal.trace-recorder.kernel.version")]
    KernelVersion,
    #[display(fmt = "timeline.internal.trace-recorder.kernel.port")]
    KernelPort,
    #[display(fmt = "timeline.internal.trace-recorder.endianness")]
    Endianness,
    #[display(fmt = "timeline.internal.trace-recorder.minor_version")]
    MinorVersion,
    #[display(fmt = "timeline.internal.trace-recorder.irq_priority_order")]
    IrqPriorityOrder,
    #[display(fmt = "timeline.internal.trace-recorder.file_size")]
    FileSize,
    #[display(fmt = "timeline.internal.trace-recorder.num_events")]
    NumEvents,
    #[display(fmt = "timeline.internal.trace-recorder.max_events")]
    MaxEvents,
    #[display(fmt = "timeline.internal.trace-recorder.buffer_full")]
    BufferFull,
    #[display(fmt = "timeline.internal.trace-recorder.frequency")]
    Frequency,
    #[display(fmt = "timeline.internal.trace-recorder.abs_time_last_event")]
    AbsTimeLastEvent,
    #[display(fmt = "timeline.internal.trace-recorder.abs_time_last_event_second")]
    AbsTimeLastEventSecond,
    #[display(fmt = "timeline.internal.trace-recorder.recorder_active")]
    RecorderActive,
    #[display(fmt = "timeline.internal.trace-recorder.isr_tail_chaining_threshold")]
    IsrChainingThreshold,
    #[display(fmt = "timeline.internal.trace-recorder.heap_mem_usage")]
    HeapMemUsage,
    #[display(fmt = "timeline.internal.trace-recorder.using_16bit_handles")]
    Using16bitHandles,
    #[display(fmt = "timeline.internal.trace-recorder.float_encoding")]
    FloatEncoding,
    #[display(fmt = "timeline.internal.trace-recorder.internal_error_occured")]
    InternalErrorOccured,
    #[display(fmt = "timeline.internal.trace-recorder.system_info")]
    SystemInfo,
}

impl TimelineAttrKey {
    pub fn enumerate() -> &'static [Self] {
        use TimelineAttrKey::*;
        &[
            Name,
            Id,
            Description,
            RunId,
            TimeDomain,
            TimeResolution,
            KernelVersion,
            KernelPort,
            Endianness,
            MinorVersion,
            IrqPriorityOrder,
            FileSize,
            NumEvents,
            MaxEvents,
            BufferFull,
            Frequency,
            AbsTimeLastEvent,
            AbsTimeLastEventSecond,
            RecorderActive,
            IsrChainingThreshold,
            HeapMemUsage,
            Using16bitHandles,
            FloatEncoding,
            InternalErrorOccured,
            SystemInfo,
        ]
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum EventAttrKey {
    #[display(fmt = "event.name")]
    Name,
    #[display(fmt = "event.timestamp")]
    Timestamp,
    #[display(fmt = "event.interaction.remote_timeline_id")]
    RemoteTimelineId,
    #[display(fmt = "event.interaction.remote_timestamp")]
    RemoteTimestamp,

    #[display(fmt = "event.internal.trace-recorder.code")]
    EventCode,
    #[display(fmt = "event.internal.trace-recorder.type")]
    EventType,
    #[display(fmt = "event.internal.trace-recorder.timestamp.ticks")]
    TimestampTicks,

    #[display(fmt = "event.internal.trace-recorder.isr.name")]
    IsrName,
    #[display(fmt = "event.internal.trace-recorder.isr.priority")]
    IsrPriority,

    #[display(fmt = "event.internal.trace-recorder.task.name")]
    TaskName,
    #[display(fmt = "event.internal.trace-recorder.task.state")]
    TaskState,
    #[display(fmt = "event.internal.trace-recorder.task.priority")]
    TaskPriority,

    #[display(fmt = "event.internal.trace-recorder.user.channel")]
    UserChannel,
    #[display(fmt = "event.internal.trace-recorder.user.formatted_string")]
    UserFormattedString,
    #[display(fmt = "event.internal.trace-recorder.user.arg0")]
    UserArg0,
    #[display(fmt = "event.internal.trace-recorder.user.arg1")]
    UserArg1,
    #[display(fmt = "event.internal.trace-recorder.user.arg2")]
    UserArg2,
    #[display(fmt = "event.internal.trace-recorder.user.arg3")]
    UserArg3,
    #[display(fmt = "event.internal.trace-recorder.user.arg4")]
    UserArg4,
    #[display(fmt = "event.internal.trace-recorder.user.arg5")]
    UserArg5,
    #[display(fmt = "event.internal.trace-recorder.user.arg6")]
    UserArg6,
    #[display(fmt = "event.internal.trace-recorder.user.arg7")]
    UserArg7,
    #[display(fmt = "event.internal.trace-recorder.user.arg8")]
    UserArg8,
    #[display(fmt = "event.internal.trace-recorder.user.arg9")]
    UserArg9,
    #[display(fmt = "event.internal.trace-recorder.user.arg10")]
    UserArg10,
    #[display(fmt = "event.internal.trace-recorder.user.arg11")]
    UserArg11,
    #[display(fmt = "event.internal.trace-recorder.user.arg12")]
    UserArg12,
    #[display(fmt = "event.internal.trace-recorder.user.arg13")]
    UserArg13,
    #[display(fmt = "event.internal.trace-recorder.user.arg14")]
    UserArg14,
}

pub type TimelineAttrKeys = AttrKeys<TimelineAttrKey>;
pub type EventAttrKeys = AttrKeys<EventAttrKey>;

#[derive(Clone, Debug)]
pub struct AttrKeys<T: Hash + Eq + std::fmt::Display>(HashMap<T, AttrKey>);

impl<T: Hash + Eq + std::fmt::Display> Default for AttrKeys<T> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<T: Hash + Eq + std::fmt::Display> AttrKeys<T> {
    pub async fn get(
        &mut self,
        client: &mut IngestClient<BoundTimelineState>,
        key: T,
    ) -> Result<AttrKey, IngestError> {
        if let Some(k) = self.0.get(&key) {
            Ok(*k)
        } else {
            let interned_key = client.attr_key(key.to_string()).await?;
            self.0.insert(key, interned_key);
            Ok(interned_key)
        }
    }
}

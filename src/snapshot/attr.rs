use crate::attr::{AttrKeyIndex, CommonEventAttrKey, CommonTimelineAttrKey};
use derive_more::Display;
use std::hash::Hash;

impl AttrKeyIndex for TimelineAttrKey {}
impl AttrKeyIndex for EventAttrKey {}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum TimelineAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonTimelineAttrKey),

    #[display(fmt = "timeline.internal.trace_recorder.minor_version")]
    MinorVersion,
    #[display(fmt = "timeline.internal.trace_recorder.file_size")]
    FileSize,
    #[display(fmt = "timeline.internal.trace_recorder.num_events")]
    NumEvents,
    #[display(fmt = "timeline.internal.trace_recorder.max_events")]
    MaxEvents,
    #[display(fmt = "timeline.internal.trace_recorder.buffer_full")]
    BufferFull,
    #[display(fmt = "timeline.internal.trace_recorder.abs_time_last_event")]
    AbsTimeLastEvent,
    #[display(fmt = "timeline.internal.trace_recorder.abs_time_last_event_second")]
    AbsTimeLastEventSecond,
    #[display(fmt = "timeline.internal.trace_recorder.recorder_active")]
    RecorderActive,
    #[display(fmt = "timeline.internal.trace_recorder.heap_mem_usage")]
    HeapMemUsage,
    #[display(fmt = "timeline.internal.trace_recorder.using_16bit_handles")]
    Using16bitHandles,
    #[display(fmt = "timeline.internal.trace_recorder.float_encoding")]
    FloatEncoding,
    #[display(fmt = "timeline.internal.trace_recorder.internal_error_occured")]
    InternalErrorOccured,
    #[display(fmt = "timeline.internal.trace_recorder.system_info")]
    SystemInfo,
}

impl From<CommonTimelineAttrKey> for TimelineAttrKey {
    fn from(c: CommonTimelineAttrKey) -> Self {
        TimelineAttrKey::Common(c)
    }
}

impl TimelineAttrKey {
    pub fn enumerate() -> &'static [Self] {
        use CommonTimelineAttrKey::*;
        use TimelineAttrKey::*;
        &[
            Common(Name),
            Common(Description),
            Common(RunId),
            Common(TimeDomain),
            Common(TimeResolution),
            Common(ClockStyle),
            Common(Protocol),
            Common(KernelVersion),
            Common(KernelPort),
            Common(Endianness),
            Common(IrqPriorityOrder),
            Common(Frequency),
            Common(IsrChainingThreshold),
            MinorVersion,
            FileSize,
            NumEvents,
            MaxEvents,
            BufferFull,
            AbsTimeLastEvent,
            AbsTimeLastEventSecond,
            RecorderActive,
            HeapMemUsage,
            Using16bitHandles,
            FloatEncoding,
            InternalErrorOccured,
            SystemInfo,
        ]
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum EventAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonEventAttrKey),

    #[display(fmt = "event.internal.trace_recorder.task.state")]
    TaskState,
}

impl From<CommonEventAttrKey> for EventAttrKey {
    fn from(c: CommonEventAttrKey) -> Self {
        EventAttrKey::Common(c)
    }
}

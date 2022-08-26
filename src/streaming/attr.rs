use crate::attr::{AttrKeyIndex, CommonEventAttrKey, CommonTimelineAttrKey};
use derive_more::Display;
use std::hash::Hash;

impl AttrKeyIndex for TimelineAttrKey {}
impl AttrKeyIndex for EventAttrKey {}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum TimelineAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonTimelineAttrKey),

    #[display(fmt = "timeline.internal.trace-recorder.format_version")]
    FormatVersion,
    #[display(fmt = "timeline.internal.trace-recorder.heap_counter")]
    HeapCounter,
    #[display(fmt = "timeline.internal.trace-recorder.session_counter")]
    SessionCounter,
    #[display(fmt = "timeline.internal.trace-recorder.tick_rate_hz")]
    TickRateHz,
    #[display(fmt = "timeline.internal.trace-recorder.hwtc_type")]
    HwTcType,
    #[display(fmt = "timeline.internal.trace-recorder.htc_period")]
    HtcPeriod,
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
            Common(Protocol),
            Common(KernelVersion),
            Common(KernelPort),
            Common(Endianness),
            Common(IrqPriorityOrder),
            Common(Frequency),
            Common(IsrChainingThreshold),
            FormatVersion,
            HeapCounter,
            SessionCounter,
            TickRateHz,
            HwTcType,
            HtcPeriod,
        ]
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum EventAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonEventAttrKey),

    #[display(fmt = "event.internal.trace-recorder.id")]
    EventId,
    #[display(fmt = "event.internal.trace-recorder.event_count")]
    EventCount,
    #[display(fmt = "event.internal.trace-recorder.parameter_count")]
    ParameterCount,

    #[display(fmt = "event.internal.trace-recorder.timer.ticks")]
    TimerTicks,
}

impl From<CommonEventAttrKey> for EventAttrKey {
    fn from(c: CommonEventAttrKey) -> Self {
        EventAttrKey::Common(c)
    }
}

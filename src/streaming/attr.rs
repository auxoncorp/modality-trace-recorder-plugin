use crate::attr::{AttrKeyIndex, CommonEventAttrKey, CommonTimelineAttrKey};
use derive_more::Display;
use std::hash::Hash;

impl AttrKeyIndex for TimelineAttrKey {}
impl AttrKeyIndex for EventAttrKey {}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum TimelineAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonTimelineAttrKey),

    #[display(fmt = "timeline.internal.trace_recorder.format_version")]
    FormatVersion,
    #[display(fmt = "timeline.internal.trace_recorder.cores")]
    NumCores,
    #[display(fmt = "timeline.internal.trace_recorder.platform_cfg")]
    PlatformCfg,
    #[display(fmt = "timeline.internal.trace_recorder.platform_cfg.version")]
    PlatformCfgVersion,
    #[display(fmt = "timeline.internal.trace_recorder.platform_cfg.version.major")]
    PlatformCfgVersionMajor,
    #[display(fmt = "timeline.internal.trace_recorder.platform_cfg.version.minor")]
    PlatformCfgVersionMinor,
    #[display(fmt = "timeline.internal.trace_recorder.platform_cfg.version.patch")]
    PlatformCfgVersionPatch,
    #[display(fmt = "timeline.internal.trace_recorder.heap.max")]
    HeapSize,
    #[display(fmt = "timeline.internal.trace_recorder.timer.type")]
    TimerType,
    #[display(fmt = "timeline.internal.trace_recorder.timer.frequency")]
    TimerFreq,
    #[display(fmt = "timeline.internal.trace_recorder.timer.period")]
    TimerPeriod,
    #[display(fmt = "timeline.internal.trace_recorder.timer.wraparounds")]
    TimerWraps,
    #[display(fmt = "timeline.internal.trace_recorder.os_tick.rate_hz")]
    TickRateHz,
    #[display(fmt = "timeline.internal.trace_recorder.os_tick.count")]
    TickCount,
    #[display(fmt = "timeline.internal.trace_recorder.latest_timestamp.ticks")]
    LatestTimestampTicks,
    #[display(fmt = "timeline.internal.trace_recorder.latest_timestamp")]
    LatestTimestamp,
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
            FormatVersion,
            NumCores,
            PlatformCfg,
            PlatformCfgVersion,
            PlatformCfgVersionMajor,
            PlatformCfgVersionMinor,
            PlatformCfgVersionPatch,
            HeapSize,
            TickRateHz,
            TimerType,
            TimerFreq,
            TimerPeriod,
            TimerWraps,
            TickRateHz,
            TickCount,
            LatestTimestampTicks,
            LatestTimestamp,
        ]
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum EventAttrKey {
    #[display(fmt = "{_0}")]
    Common(CommonEventAttrKey),

    #[display(fmt = "event.internal.trace_recorder.id")]
    EventId,
    #[display(fmt = "event.internal.trace_recorder.event_count")]
    EventCount,
    #[display(fmt = "event.internal.trace_recorder.parameter_count")]
    ParameterCount,

    #[display(fmt = "event.internal.trace_recorder.timer.ticks")]
    TimerTicks,
}

impl From<CommonEventAttrKey> for EventAttrKey {
    fn from(c: CommonEventAttrKey) -> Self {
        EventAttrKey::Common(c)
    }
}

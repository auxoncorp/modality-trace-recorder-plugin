use auxon_sdk::{api::AttrKey, ingest_protocol::InternedAttrKey};
use derive_more::Display;
use std::collections::HashMap;

pub type AttrKeys<T> = HashMap<T, InternedAttrKey>;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum TimelineAttrKey {
    #[display(fmt = "timeline.name")]
    Name,
    #[display(fmt = "timeline.description")]
    Description,
    #[display(fmt = "timeline.run_id")]
    RunId,
    #[display(fmt = "timeline.time_domain")]
    TimeDomain,
    #[display(fmt = "timeline.time_resolution")]
    TimeResolution,
    #[display(fmt = "timeline.clock_style")]
    ClockStyle,

    #[display(fmt = "timeline.internal.trace_recorder.protocol")]
    Protocol,
    #[display(fmt = "timeline.internal.trace_recorder.kernel.version")]
    KernelVersion,
    #[display(fmt = "timeline.internal.trace_recorder.kernel.port")]
    KernelPort,
    #[display(fmt = "timeline.internal.trace_recorder.endianness")]
    Endianness,
    #[display(fmt = "timeline.internal.trace_recorder.irq_priority_order")]
    IrqPriorityOrder,
    #[display(fmt = "timeline.internal.trace_recorder.frequency")]
    Frequency,
    #[display(fmt = "timeline.internal.trace_recorder.isr_tail_chaining_threshold")]
    IsrChainingThreshold,

    #[display(fmt = "timeline.internal.trace_recorder.object_handle")]
    ObjectHandle,
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

    #[display(fmt = "timeline.trace_recorder.plugin.version")]
    PluginVersion,
    #[display(fmt = "timeline.trace_recorder.import.file")]
    ImportFile,
    #[display(fmt = "timeline.trace_recorder.tcp_collector.remote")]
    TcpRemote,
    #[display(fmt = "timeline.internal.trace_recorder.interaction_mode")]
    InteractionMode,

    #[display(fmt = "timeline.internal.trace_recorder.cpu_utilization.measurement_window.ticks")]
    CpuUtilizationMeasurementWindowTicks,
    #[display(fmt = "timeline.internal.trace_recorder.cpu_utilization.measurement_window")]
    CpuUtilizationMeasurementWindow,

    #[display(fmt = "timeline.{_0}")]
    Custom(String),
}

impl TimelineAttrKey {
    pub fn into_cfg_attr(&self) -> AttrKey {
        AttrKey::new(self.to_string().trim_start_matches("timeline.").to_owned())
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum EventAttrKey {
    #[display(fmt = "event.name")]
    Name,
    #[display(fmt = "event.timestamp")]
    Timestamp,
    #[display(fmt = "event.interaction.remote_timeline_id")]
    RemoteTimelineId,
    #[display(fmt = "event.interaction.remote_nonce")]
    RemoteNonce,
    #[display(fmt = "event.internal.trace_recorder.nonce")]
    InternalNonce,
    #[display(fmt = "event.nonce")]
    Nonce,
    #[display(fmt = "event.mutator.id")]
    MutatorId,
    #[display(fmt = "event.internal.trace_recorder.mutator.id")]
    InternalMutatorId,
    #[display(fmt = "event.mutation.id")]
    MutationId,
    #[display(fmt = "event.internal.trace_recorder.mutation.id")]
    InternalMutationId,
    #[display(fmt = "event.mutation.success")]
    MutationSuccess,

    #[display(fmt = "event.internal.trace_recorder.id")]
    EventId,
    #[display(fmt = "event.internal.trace_recorder.event_count")]
    EventCount,
    #[display(fmt = "event.internal.trace_recorder.event_count.raw")]
    EventCountRaw,
    #[display(fmt = "event.trace_recorder.dropped_preceding_events")]
    DroppedEvents,
    #[display(fmt = "event.internal.trace_recorder.parameter_count")]
    ParameterCount,

    #[display(fmt = "event.internal.trace_recorder.code")]
    EventCode,
    #[display(fmt = "event.internal.trace_recorder.type")]
    EventType,

    #[display(fmt = "event.internal.trace_recorder.timestamp.ticks")]
    TimestampTicks,
    #[display(fmt = "event.internal.trace_recorder.timer.ticks")]
    TimerTicks,

    #[display(fmt = "event.internal.trace_recorder.object_handle")]
    ObjectHandle,

    #[display(fmt = "event.isr")]
    IsrName,
    #[display(fmt = "event.priority")]
    IsrPriority,

    #[display(fmt = "event.task")]
    TaskName,
    #[display(fmt = "event.priority")]
    TaskPriority,

    #[display(fmt = "event.address")]
    MemoryAddress,
    #[display(fmt = "event.size")]
    MemorySize,
    #[display(fmt = "event.internal.trace_recorder.heap.current")]
    MemoryHeapCurrent,
    #[display(fmt = "event.internal.trace_recorder.heap.high_mark")]
    MemoryHeapHighMark,
    #[display(fmt = "event.internal.trace_recorder.heap.max")]
    MemoryHeapMax,

    #[display(fmt = "event.low_mark")]
    StackLowMark,

    #[display(fmt = "event.queue")]
    QueueName,
    #[display(fmt = "event.queue_length")]
    QueueLength,
    #[display(fmt = "event.messages_waiting")]
    QueueMessagesWaiting,

    #[display(fmt = "event.mutex")]
    MutexName,

    #[display(fmt = "event.semaphore")]
    SemaphoreName,
    #[display(fmt = "event.count")]
    SemaphoreCount,

    #[display(fmt = "event.event_group")]
    EventGroupName,
    #[display(fmt = "event.bits")]
    EventGroupBits,

    #[display(fmt = "event.message_buffer")]
    MessageBufferName,
    #[display(fmt = "event.buffer_size")]
    MessageBufferSize,
    #[display(fmt = "event.bytes_in_buffer")]
    MessageBufferBytesInBuffer,

    #[display(fmt = "event.internal.trace_recorder.ticks_to_wait")]
    TicksToWait,
    #[display(fmt = "event.internal.trace_recorder.ns_to_wait")]
    NanosToWait,

    #[display(fmt = "event.channel")]
    UserChannel,
    #[display(fmt = "event.formatted_string")]
    UserFormattedString,
    #[display(fmt = "event.arg0")]
    UserArg0,
    #[display(fmt = "event.arg1")]
    UserArg1,
    #[display(fmt = "event.arg2")]
    UserArg2,
    #[display(fmt = "event.arg3")]
    UserArg3,
    #[display(fmt = "event.arg4")]
    UserArg4,
    #[display(fmt = "event.arg5")]
    UserArg5,
    #[display(fmt = "event.arg6")]
    UserArg6,
    #[display(fmt = "event.arg7")]
    UserArg7,
    #[display(fmt = "event.arg8")]
    UserArg8,
    #[display(fmt = "event.arg9")]
    UserArg9,
    #[display(fmt = "event.arg10")]
    UserArg10,
    #[display(fmt = "event.arg11")]
    UserArg11,
    #[display(fmt = "event.arg12")]
    UserArg12,
    #[display(fmt = "event.arg13")]
    UserArg13,
    #[display(fmt = "event.arg14")]
    UserArg14,

    #[display(fmt = "event.internal.trace_recorder.total_runtime.ticks")]
    TotalRuntimeTicks,
    #[display(fmt = "event.total_runtime")]
    TotalRuntime,
    #[display(fmt = "event.internal.trace_recorder.runtime.ticks")]
    RuntimeTicks,
    #[display(fmt = "event.runtime")]
    Runtime,
    #[display(fmt = "event.internal.trace_recorder.runtime_window.ticks")]
    RuntimeWindowTicks,
    #[display(fmt = "event.runtime_window")]
    RuntimeWindow,
    #[display(fmt = "event.internal.trace_recorder.runtime_in_window.ticks")]
    RuntimeInWindowTicks,
    #[display(fmt = "event.runtime_in_window")]
    RuntimeInWindow,
    #[display(fmt = "event.cpu_utilization")]
    CpuUtilization,

    #[display(fmt = "event.{_0}")]
    CustomUserArg(String),
}

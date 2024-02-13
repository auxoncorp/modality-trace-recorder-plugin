use derive_more::Display;
use modality_ingest_client::{BoundTimelineState, IngestClient, IngestError};
use modality_ingest_protocol::InternedAttrKey;
use std::{collections::HashMap, fmt, hash::Hash};

pub trait AttrKeyIndex: Hash + Eq + Clone + fmt::Display {}

#[derive(Clone, Debug)]
pub struct AttrKeys<T: AttrKeyIndex>(HashMap<T, InternedAttrKey>);

impl<T: AttrKeyIndex> Default for AttrKeys<T> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<T: AttrKeyIndex> AttrKeys<T> {
    pub async fn get(
        &mut self,
        client: &mut IngestClient<BoundTimelineState>,
        key: T,
    ) -> Result<InternedAttrKey, IngestError> {
        if let Some(k) = self.0.get(&key) {
            Ok(*k)
        } else {
            let interned_key = client.declare_attr_key(key.to_string()).await?;
            self.0.insert(key, interned_key);
            Ok(interned_key)
        }
    }

    pub(crate) fn remove_string_key_entry(&mut self, key: &str) -> Option<(T, InternedAttrKey)> {
        self.0
            .keys()
            .find_map(|k| {
                let k_str = k.to_string();
                if k_str.as_str() == key {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .and_then(|k| self.0.remove_entry(&k))
    }

    pub(crate) fn insert(&mut self, key: T, interned_key: InternedAttrKey) {
        self.0.insert(key, interned_key);
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum CommonTimelineAttrKey {
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

    #[display(fmt = "timeline.{_0}")]
    Custom(String),
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum CommonEventAttrKey {
    #[display(fmt = "event.name")]
    Name,
    #[display(fmt = "event.timestamp")]
    Timestamp,
    #[display(fmt = "event.interaction.remote_timeline_id")]
    RemoteTimelineId,
    #[display(fmt = "event.interaction.remote_timestamp")]
    RemoteTimestamp,
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

    #[display(fmt = "event.internal.trace_recorder.code")]
    EventCode,
    #[display(fmt = "event.internal.trace_recorder.type")]
    EventType,
    #[display(fmt = "event.internal.trace_recorder.timestamp.ticks")]
    TimestampTicks,

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

    #[display(fmt = "event.semaphore")]
    SemaphoreName,
    #[display(fmt = "event.count")]
    SemaphoreCount,

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
    #[display(fmt = "event.{_0}")]
    CustomUserArg(String),
}

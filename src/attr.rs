use derive_more::Display;
use modality_ingest_client::{types::AttrKey, BoundTimelineState, IngestClient, IngestError};
use std::{collections::HashMap, fmt, hash::Hash};

pub trait AttrKeyIndex: Hash + Eq + fmt::Display {}

#[derive(Clone, Debug)]
pub struct AttrKeys<T: AttrKeyIndex>(HashMap<T, AttrKey>);

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

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
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

    #[display(fmt = "timeline.internal.trace-recorder.protocol")]
    Protocol,
    #[display(fmt = "timeline.internal.trace-recorder.kernel.version")]
    KernelVersion,
    #[display(fmt = "timeline.internal.trace-recorder.kernel.port")]
    KernelPort,
    #[display(fmt = "timeline.internal.trace-recorder.endianness")]
    Endianness,
    #[display(fmt = "timeline.internal.trace-recorder.irq_priority_order")]
    IrqPriorityOrder,
    #[display(fmt = "timeline.internal.trace-recorder.frequency")]
    Frequency,
    #[display(fmt = "timeline.internal.trace-recorder.isr_tail_chaining_threshold")]
    IsrChainingThreshold,
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
    #[display(fmt = "event.internal.trace-recorder.task.priority")]
    TaskPriority,

    // User events are more important so we surface the attrs at the top level
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

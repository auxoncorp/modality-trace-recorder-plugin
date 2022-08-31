use modality_api::TimelineId;
use std::hash::Hash;
use trace_recorder_parser::{snapshot, streaming, time::Timestamp, types::ObjectHandle};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum ContextSwitchOutcome {
    /// Current task/ISR is already marked as active, we're on the same timeline
    Same,
    /// Switched to a new task/ISR, return the remote (previous) timeline ID and
    /// the last event timestamped logged on the previous timeline
    Different(TimelineId, Timestamp),
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum ContextHandle {
    Task(ObjectHandle),
    Isr(ObjectHandle),
}

impl ContextHandle {
    pub fn object_handle(self) -> ObjectHandle {
        match self {
            ContextHandle::Task(h) => h,
            ContextHandle::Isr(h) => h,
        }
    }
}

impl From<snapshot::event::TaskEvent> for ContextHandle {
    fn from(event: snapshot::event::TaskEvent) -> Self {
        ContextHandle::Task(event.handle)
    }
}

impl From<snapshot::event::IsrEvent> for ContextHandle {
    fn from(event: snapshot::event::IsrEvent) -> Self {
        ContextHandle::Isr(event.handle)
    }
}

impl From<streaming::event::TaskEvent> for ContextHandle {
    fn from(event: streaming::event::TaskEvent) -> Self {
        ContextHandle::Task(event.handle)
    }
}

impl From<streaming::event::IsrEvent> for ContextHandle {
    fn from(event: streaming::event::IsrEvent) -> Self {
        ContextHandle::Isr(event.handle)
    }
}

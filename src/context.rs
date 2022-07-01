use modality_ingest_client::types::TimelineId;
use std::hash::Hash;
use trace_recorder_parser::snapshot::{
    event::{IsrEvent, TaskEvent},
    object_properties::ObjectHandle,
    Timestamp,
};

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

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum ContextEvent {
    Task(TaskEvent),
    Isr(IsrEvent),
}

impl ContextEvent {
    pub fn handle(&self) -> ContextHandle {
        match self {
            ContextEvent::Task(event) => ContextHandle::Task(event.handle),
            ContextEvent::Isr(event) => ContextHandle::Isr(event.handle),
        }
    }
}

impl From<TaskEvent> for ContextEvent {
    fn from(event: TaskEvent) -> Self {
        ContextEvent::Task(event)
    }
}

impl From<IsrEvent> for ContextEvent {
    fn from(event: IsrEvent) -> Self {
        ContextEvent::Isr(event)
    }
}

use crate::{
    attr::{EventAttrKey, TimelineAttrKey},
    config::{PluginConfig, TraceRecorderConfig},
    error::Error,
    PLUGIN_VERSION,
};
use async_trait::async_trait;
use auxon_sdk::{
    api::{AttrVal, Nanoseconds},
    reflector_config::AttrKeyEqValuePair,
};
use derive_more::Display;
use std::collections::HashMap;
use trace_recorder_parser::{
    streaming::{
        event::{Event, EventCode, IsrEvent, TaskEvent},
        RecorderData,
    },
    time::{Frequency, Timestamp},
    types::{Argument, ObjectClass, ObjectHandle, STARTUP_TASK_NAME, UNNAMED_OBJECT},
};
use tracing::{trace, warn};
use uuid::Uuid;

pub trait NanosecondsExt {
    const ONE_SECOND: u64 = 1_000_000_000;

    fn resolution_ns(&self) -> Option<Nanoseconds>;

    fn timer_frequency(&self) -> Option<u64>;

    /// Convert to nanosecond time base using the frequency if non-zero
    fn convert_timestamp<T: Into<Timestamp>>(&self, ticks: T) -> Option<Nanoseconds> {
        let t = ticks.into();
        self.timer_frequency()
            .map(|freq| Nanoseconds::from((t.get_raw() * Self::ONE_SECOND) / freq))
    }
}

impl NanosecondsExt for Frequency {
    fn timer_frequency(&self) -> Option<u64> {
        if self.is_unitless() {
            None
        } else {
            Some(self.get_raw().into())
        }
    }

    /// Returns ~ns/tick (note that this will truncate)
    fn resolution_ns(&self) -> Option<Nanoseconds> {
        if self.is_unitless() {
            None
        } else {
            Nanoseconds::from(Self::ONE_SECOND / u64::from(self.get_raw())).into()
        }
    }
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

impl From<&TaskEvent> for ContextHandle {
    fn from(event: &TaskEvent) -> Self {
        ContextHandle::Task(event.handle)
    }
}

impl From<&IsrEvent> for ContextHandle {
    fn from(event: &IsrEvent) -> Self {
        ContextHandle::Isr(event.handle)
    }
}

pub type EventAttributes = HashMap<EventAttrKey, AttrVal>;
pub type TimelineAttributes = HashMap<TimelineAttrKey, AttrVal>;

#[async_trait]
pub trait RecorderDataExt {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error>;

    fn object_handle(&self, obj_name: &str) -> Option<ObjectHandle>;

    fn add_core_event_attributes(
        &self,
        cfg: &PluginConfig,
        event_code: EventCode,
        event: &Event,
        attrs: &mut EventAttributes,
    ) -> Result<(), Error>;

    fn add_core_timeline_attributes(
        &self,
        cfg: &PluginConfig,
        handle: ContextHandle,
        attrs: &mut TimelineAttributes,
    ) -> Result<(), Error>;

    fn common_timeline_attributes(&self, cfg: &TraceRecorderConfig) -> TimelineAttributes;
}

#[async_trait]
impl RecorderDataExt for RecorderData {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error> {
        Ok(ObjectHandle::NO_TASK)
    }

    fn object_handle(&self, obj_name: &str) -> Option<ObjectHandle> {
        self.entry_table
            .symbol_handle(obj_name, Some(ObjectClass::Task))
            .or(self
                .entry_table
                .symbol_handle(obj_name, Some(ObjectClass::Isr)))
    }

    fn add_core_event_attributes(
        &self,
        cfg: &PluginConfig,
        event_code: EventCode,
        event: &Event,
        attrs: &mut EventAttributes,
    ) -> Result<(), Error> {
        let event_type = event_code.event_type();
        let event_id = event_code.event_id();
        let parameter_count = event_code.parameter_count();

        attrs.insert(EventAttrKey::Name, event_type.to_string().into());
        attrs.insert(
            EventAttrKey::EventCode,
            AttrVal::Integer(u16::from(event_code).into()),
        );
        attrs.insert(EventAttrKey::EventType, event_type.to_string().into());
        attrs.insert(
            EventAttrKey::EventId,
            AttrVal::Integer(u16::from(event_id).into()),
        );
        attrs.insert(
            EventAttrKey::ParameterCount,
            AttrVal::Integer(u8::from(parameter_count).into()),
        );

        match event {
            Event::TraceStart(ev) => {
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.current_task_handle).into()),
                );
                attrs.insert(EventAttrKey::TaskName, ev.current_task.to_string().into());
            }

            Event::IsrBegin(ev) | Event::IsrResume(ev) | Event::IsrDefine(ev) => {
                attrs.insert(EventAttrKey::IsrName, ev.name.to_string().into());
                attrs.insert(
                    EventAttrKey::IsrPriority,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
            }

            Event::TaskBegin(ev)
            | Event::TaskReady(ev)
            | Event::TaskResume(ev)
            | Event::TaskCreate(ev)
            | Event::TaskActivate(ev)
            | Event::TaskPriority(ev)
            | Event::TaskPriorityInherit(ev)
            | Event::TaskPriorityDisinherit(ev) => {
                attrs.insert(EventAttrKey::TaskName, ev.name.to_string().into());
                attrs.insert(
                    EventAttrKey::TaskPriority,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
            }

            Event::TaskNotify(ev)
            | Event::TaskNotifyFromIsr(ev)
            | Event::TaskNotifyWait(ev)
            | Event::TaskNotifyWaitBlock(ev) => {
                if let Some(name) = ev.task_name.as_ref() {
                    attrs.insert(
                        EventAttrKey::TaskName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        EventAttrKey::TicksToWait,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    if let Some(ns) = self
                        .timestamp_info
                        .timer_frequency
                        .convert_timestamp(ticks_to_wait)
                    {
                        attrs.insert(EventAttrKey::NanosToWait, ns.into());
                    }
                }
            }

            Event::MemoryAlloc(ev) | Event::MemoryFree(ev) => {
                attrs.insert(
                    EventAttrKey::MemoryAddress,
                    AttrVal::Integer(ev.address.into()),
                );
                attrs.insert(EventAttrKey::MemorySize, AttrVal::Integer(ev.size.into()));
                attrs.insert(
                    EventAttrKey::MemoryHeapCurrent,
                    AttrVal::Integer(ev.heap.current.into()),
                );
                attrs.insert(
                    EventAttrKey::MemoryHeapHighMark,
                    AttrVal::Integer(ev.heap.high_water_mark.into()),
                );
                attrs.insert(
                    EventAttrKey::MemoryHeapMax,
                    AttrVal::Integer(ev.heap.max.into()),
                );
            }

            Event::UnusedStack(ev) => {
                attrs.insert(EventAttrKey::TaskName, ev.task.to_string().into());
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::StackLowMark,
                    AttrVal::Integer(ev.low_mark.into()),
                );
            }

            Event::QueueCreate(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::QueueName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::QueueLength,
                    AttrVal::Integer(ev.queue_length.into()),
                );
            }

            Event::QueueSend(ev)
            | Event::QueueSendBlock(ev)
            | Event::QueueSendFromIsr(ev)
            | Event::QueueReceive(ev)
            | Event::QueueReceiveBlock(ev)
            | Event::QueueReceiveFromIsr(ev)
            | Event::QueuePeek(ev)
            | Event::QueuePeekBlock(ev)
            | Event::QueueSendFront(ev)
            | Event::QueueSendFrontBlock(ev)
            | Event::QueueSendFrontFromIsr(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::QueueName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::QueueMessagesWaiting,
                    AttrVal::Integer(ev.messages_waiting.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        EventAttrKey::TicksToWait,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    if let Some(ns) = self
                        .timestamp_info
                        .timer_frequency
                        .convert_timestamp(ticks_to_wait)
                    {
                        attrs.insert(EventAttrKey::NanosToWait, ns.into());
                    }
                }
            }

            Event::MutexCreate(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::MutexName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
            }

            Event::MutexGive(ev)
            | Event::MutexGiveBlock(ev)
            | Event::MutexGiveRecursive(ev)
            | Event::MutexTake(ev)
            | Event::MutexTakeBlock(ev)
            | Event::MutexTakeRecursive(ev)
            | Event::MutexTakeRecursiveBlock(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::MutexName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        EventAttrKey::TicksToWait,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    if let Some(ns) = self
                        .timestamp_info
                        .timer_frequency
                        .convert_timestamp(ticks_to_wait)
                    {
                        attrs.insert(EventAttrKey::NanosToWait, ns.into());
                    }
                }
            }

            Event::SemaphoreBinaryCreate(ev) | Event::SemaphoreCountingCreate(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::SemaphoreName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(count) = ev.count {
                    attrs.insert(EventAttrKey::SemaphoreCount, AttrVal::Integer(count.into()));
                }
            }

            Event::SemaphoreGive(ev)
            | Event::SemaphoreGiveBlock(ev)
            | Event::SemaphoreGiveFromIsr(ev)
            | Event::SemaphoreTake(ev)
            | Event::SemaphoreTakeBlock(ev)
            | Event::SemaphoreTakeFromIsr(ev)
            | Event::SemaphorePeek(ev)
            | Event::SemaphorePeekBlock(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::SemaphoreName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::SemaphoreCount,
                    AttrVal::Integer(ev.count.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        EventAttrKey::TicksToWait,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    if let Some(ns) = self
                        .timestamp_info
                        .timer_frequency
                        .convert_timestamp(ticks_to_wait)
                    {
                        attrs.insert(EventAttrKey::NanosToWait, ns.into());
                    }
                }
            }

            Event::EventGroupCreate(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::EventGroupName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::EventGroupBits,
                    AttrVal::Integer(ev.event_bits.into()),
                );
            }

            Event::EventGroupSync(ev)
            | Event::EventGroupWaitBits(ev)
            | Event::EventGroupClearBits(ev)
            | Event::EventGroupClearBitsFromIsr(ev)
            | Event::EventGroupSetBits(ev)
            | Event::EventGroupSetBitsFromIsr(ev)
            | Event::EventGroupSyncBlock(ev)
            | Event::EventGroupWaitBitsBlock(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::EventGroupName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::EventGroupBits,
                    AttrVal::Integer(ev.bits.into()),
                );
            }

            Event::MessageBufferCreate(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::MessageBufferName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::MessageBufferSize,
                    AttrVal::Integer(ev.buffer_size.into()),
                );
            }

            Event::MessageBufferSend(ev)
            | Event::MessageBufferSendFromIsr(ev)
            | Event::MessageBufferReceive(ev)
            | Event::MessageBufferReceiveFromIsr(ev)
            | Event::MessageBufferReset(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::MessageBufferName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::MessageBufferBytesInBuffer,
                    AttrVal::Integer(ev.bytes_in_buffer.into()),
                );
            }

            Event::MessageBufferSendBlock(ev) | Event::MessageBufferReceiveBlock(ev) => {
                if let Some(name) = ev.name.as_ref() {
                    attrs.insert(
                        EventAttrKey::MessageBufferName,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
            }

            Event::StateMachineCreate(ev) => {
                attrs.insert(
                    EventAttrKey::StateMachineName,
                    AttrVal::String(ev.name.to_string().into()),
                );
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
            }

            Event::StateMachineStateCreate(ev) | Event::StateMachineStateChange(ev) => {
                attrs.insert(
                    EventAttrKey::StateMachineName,
                    AttrVal::String(ev.name.to_string().into()),
                );
                attrs.insert(
                    EventAttrKey::ObjectHandle,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    EventAttrKey::StateMachineState,
                    AttrVal::String(ev.state.to_string().into()),
                );
                attrs.insert(
                    EventAttrKey::StateMachineStateHandle,
                    AttrVal::Integer(u32::from(ev.state_handle).into()),
                );
            }

            Event::User(ev) => {
                if cfg.user_event_channel {
                    // Use the channel as the event name
                    attrs.insert(EventAttrKey::Name, ev.channel.to_string().into());
                } else if cfg.user_event_format_string {
                    // Use the formatted string as the event name
                    attrs.insert(EventAttrKey::Name, ev.formatted_string.to_string().into());
                }

                // Handle channel event name mappings
                if let Some(name) = cfg.user_event_channel_rename_map.get(ev.channel.as_str()) {
                    attrs.insert(EventAttrKey::Name, name.to_string().into());
                } else if ev.channel.as_str() == "#WFR" {
                    warn!(
                        msg = %ev.formatted_string,
                        "Target produced a warning on the '#WFR' channel"
                    );
                    attrs.insert(EventAttrKey::Name, "WARNING_FROM_RECORDER".into());
                }

                // Handle format string event name mappings
                if let Some(name) = cfg
                    .user_event_formatted_string_rename_map
                    .get(ev.formatted_string.as_str())
                {
                    attrs.insert(EventAttrKey::Name, name.to_string().into());
                }

                attrs.insert(EventAttrKey::UserChannel, ev.channel.to_string().into());
                attrs.insert(
                    EventAttrKey::UserFormattedString,
                    ev.formatted_string.to_string().into(),
                );

                let custom_arg_keys = cfg
                    .user_event_fmt_arg_attr_keys
                    .arg_attr_keys(ev.channel.as_str(), &ev.format_string);
                if let Some(custom_arg_keys) = custom_arg_keys {
                    if custom_arg_keys.len() != ev.args.len() {
                        return Err(Error::FmtArgAttrKeysCountMismatch(
                            ev.format_string.clone().into(),
                            custom_arg_keys.to_vec(),
                        ));
                    }
                }

                for (idx, arg) in ev.args.iter().enumerate() {
                    let key = if let Some(custom_arg_keys) = custom_arg_keys {
                        // SAFETY: len checked above
                        EventAttrKey::CustomUserArg(custom_arg_keys[idx].clone())
                    } else {
                        match idx {
                            0 => EventAttrKey::UserArg0,
                            1 => EventAttrKey::UserArg1,
                            2 => EventAttrKey::UserArg2,
                            3 => EventAttrKey::UserArg3,
                            4 => EventAttrKey::UserArg4,
                            5 => EventAttrKey::UserArg5,
                            6 => EventAttrKey::UserArg6,
                            7 => EventAttrKey::UserArg7,
                            8 => EventAttrKey::UserArg8,
                            9 => EventAttrKey::UserArg9,
                            10 => EventAttrKey::UserArg10,
                            11 => EventAttrKey::UserArg11,
                            12 => EventAttrKey::UserArg12,
                            13 => EventAttrKey::UserArg13,
                            14 => EventAttrKey::UserArg14,
                            _ => return Err(Error::ExceededMaxUserEventArgs),
                        }
                    };
                    attrs.insert(key, arg_to_attr_val(arg));
                }
            }

            Event::ObjectName(_) | Event::TsConfig(_) | Event::Unknown(_) => (),
        }

        Ok(())
    }

    fn add_core_timeline_attributes(
        &self,
        cfg: &PluginConfig,
        handle: ContextHandle,
        attrs: &mut TimelineAttributes,
    ) -> Result<(), Error> {
        let obj_handle = handle.object_handle();
        let obj_class = self.entry_table.class(obj_handle);
        let obj_name = self.entry_table.symbol(obj_handle).map(|s| s.to_string());
        let (name, description) = match handle {
            ContextHandle::Task(task_handle) => {
                let obj_name = obj_name.ok_or(Error::TaskPropertiesLookup(task_handle))?;
                let name = object_name(obj_name, obj_class.into(), task_handle);
                let desc = format!(
                    "{} {} '{}'",
                    self.header.kernel_port,
                    MaybeUnknownObjectClass::from(obj_class),
                    name
                );

                let is_startup_task = name == STARTUP_TASK_NAME;
                match cfg.startup_task_name.as_ref() {
                    Some(startup_task_name) if is_startup_task => {
                        (startup_task_name.to_string(), desc)
                    }
                    _ => (name, desc),
                }
            }
            ContextHandle::Isr(isr_handle) => {
                let obj_name = obj_name.ok_or(Error::IsrPropertiesLookup(isr_handle))?;
                let name = object_name(obj_name, obj_class.into(), isr_handle);
                let desc = format!(
                    "{} {} '{}'",
                    self.header.kernel_port,
                    MaybeUnknownObjectClass::from(obj_class),
                    name
                );
                (name, desc)
            }
        };

        attrs.insert(TimelineAttrKey::Name, name.into());
        attrs.insert(TimelineAttrKey::Description, description.into());
        attrs.insert(TimelineAttrKey::ObjectHandle, u32::from(obj_handle).into());

        Ok(())
    }

    fn common_timeline_attributes(&self, cfg: &TraceRecorderConfig) -> TimelineAttributes {
        let mut attrs = HashMap::new();
        let run_id = if let Some(id) = cfg.plugin.run_id.as_ref() {
            if let Ok(val) = id.parse::<u64>() {
                val.into()
            } else {
                Uuid::new_v4().to_string().into()
            }
        } else {
            Uuid::new_v4().to_string().into()
        };
        let time_domain = if let Some(id) = cfg.plugin.time_domain.as_ref() {
            if let Ok(val) = id.parse::<u64>() {
                val.into()
            } else {
                Uuid::new_v4().to_string().into()
            }
        } else {
            Uuid::new_v4().to_string().into()
        };
        trace!(run_id = %run_id, time_domain = %time_domain);

        if let Some(r) = self.timestamp_info.timer_frequency.resolution_ns() {
            // Only have ns resolution if frequency is non-zero
            attrs.insert(TimelineAttrKey::TimeResolution, r.into());
        }

        attrs.insert(TimelineAttrKey::RunId, run_id);
        attrs.insert(TimelineAttrKey::TimeDomain, time_domain);
        attrs.insert(TimelineAttrKey::ClockStyle, "relative".into());
        attrs.insert(TimelineAttrKey::Protocol, self.protocol.to_string().into());
        attrs.insert(
            TimelineAttrKey::KernelVersion,
            self.header.kernel_version.to_string().into(),
        );
        attrs.insert(
            TimelineAttrKey::KernelPort,
            self.header.kernel_port.to_string().into(),
        );
        attrs.insert(
            TimelineAttrKey::Endianness,
            self.header.endianness.to_string().into(),
        );
        attrs.insert(
            TimelineAttrKey::IrqPriorityOrder,
            AttrVal::Integer(self.header.irq_priority_order.into()),
        );
        attrs.insert(
            TimelineAttrKey::Frequency,
            AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into()),
        );
        attrs.insert(
            TimelineAttrKey::IsrChainingThreshold,
            AttrVal::Integer(self.header.isr_tail_chaining_threshold.into()),
        );
        attrs.insert(TimelineAttrKey::PluginVersion, PLUGIN_VERSION.into());
        attrs.insert(
            TimelineAttrKey::FormatVersion,
            AttrVal::Integer(self.header.format_version.into()),
        );
        attrs.insert(
            TimelineAttrKey::NumCores,
            AttrVal::Integer(self.header.num_cores.into()),
        );
        attrs.insert(
            TimelineAttrKey::PlatformCfg,
            AttrVal::String(self.header.platform_cfg.clone().into()),
        );
        attrs.insert(
            TimelineAttrKey::PlatformCfgVersion,
            AttrVal::String(self.header.platform_cfg_version.to_string().into()),
        );
        attrs.insert(
            TimelineAttrKey::PlatformCfgVersionMajor,
            AttrVal::Integer(self.header.platform_cfg_version.major.into()),
        );
        attrs.insert(
            TimelineAttrKey::PlatformCfgVersionMinor,
            AttrVal::Integer(self.header.platform_cfg_version.minor.into()),
        );
        attrs.insert(
            TimelineAttrKey::PlatformCfgVersionPatch,
            AttrVal::Integer(self.header.platform_cfg_version.patch.into()),
        );
        attrs.insert(
            TimelineAttrKey::HeapSize,
            AttrVal::Integer(self.system_heap().max.into()),
        );
        attrs.insert(
            TimelineAttrKey::TimerType,
            self.timestamp_info.timer_type.to_string().into(),
        );
        attrs.insert(
            TimelineAttrKey::TimerFreq,
            AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into()),
        );
        attrs.insert(
            TimelineAttrKey::TimerPeriod,
            AttrVal::Integer(self.timestamp_info.timer_period.into()),
        );
        attrs.insert(
            TimelineAttrKey::TimerWraps,
            AttrVal::Integer(self.timestamp_info.timer_wraparounds.into()),
        );
        attrs.insert(
            TimelineAttrKey::TickRateHz,
            AttrVal::Integer(u32::from(self.timestamp_info.os_tick_rate_hz).into()),
        );
        attrs.insert(
            TimelineAttrKey::TickCount,
            AttrVal::Integer(self.timestamp_info.os_tick_count.into()),
        );
        attrs.insert(
            TimelineAttrKey::LatestTimestampTicks,
            self.timestamp_info.latest_timestamp.ticks().into(),
        );
        if let Some(ns) = self
            .timestamp_info
            .timer_frequency
            .convert_timestamp(self.timestamp_info.latest_timestamp)
        {
            attrs.insert(TimelineAttrKey::LatestTimestamp, ns.into());
        }
        attrs.insert(
            TimelineAttrKey::InteractionMode,
            cfg.plugin.interaction_mode.to_string().into(),
        );

        merge_cfg_attributes(
            &cfg.ingest
                .timeline_attributes
                .additional_timeline_attributes,
            &mut attrs,
        );

        merge_cfg_attributes(
            &cfg.ingest.timeline_attributes.override_timeline_attributes,
            &mut attrs,
        );

        attrs
    }
}

pub(crate) fn merge_cfg_attributes(
    attrs_to_merge: &[AttrKeyEqValuePair],
    attrs: &mut TimelineAttributes,
) {
    for kv in attrs_to_merge.iter() {
        attrs.insert(TimelineAttrKey::Custom(kv.0.to_string()), kv.1.clone());
    }
}

fn object_name(name: String, class: MaybeUnknownObjectClass, handle: ObjectHandle) -> String {
    if name.is_empty() {
        format!("{UNNAMED_OBJECT}:{class}:{handle}")
    } else {
        name
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
enum MaybeUnknownObjectClass {
    #[display(fmt = "Object")]
    Unknown,
    #[display(fmt = "{_0}")]
    Known(ObjectClass),
}

impl From<ObjectClass> for MaybeUnknownObjectClass {
    fn from(c: ObjectClass) -> Self {
        MaybeUnknownObjectClass::Known(c)
    }
}

impl From<Option<ObjectClass>> for MaybeUnknownObjectClass {
    fn from(c: Option<ObjectClass>) -> Self {
        match c {
            Some(c) => MaybeUnknownObjectClass::Known(c),
            None => MaybeUnknownObjectClass::Unknown,
        }
    }
}

fn arg_to_attr_val(arg: &Argument) -> AttrVal {
    match arg {
        Argument::I8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::F32(v) => AttrVal::from(f64::from(v.0)),
        Argument::F64(v) => AttrVal::from(v.0),
        Argument::String(v) => AttrVal::String(v.clone().into()),
    }
}

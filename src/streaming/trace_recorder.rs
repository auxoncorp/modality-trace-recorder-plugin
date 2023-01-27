use crate::streaming::{EventAttrKey, TimelineAttrKey};
use crate::trace_recorder::Error;
use crate::{
    Client, CommonTimelineAttrKey, ContextHandle, NanosecondsExt, TimelineDetails,
    TraceRecorderConfig, TraceRecorderExt,
};
use async_trait::async_trait;
use derive_more::Display;
use modality_api::{AttrVal, BigInt};
use modality_ingest_protocol::InternedAttrKey;
use std::collections::HashMap;
use trace_recorder_parser::streaming::RecorderData;
use trace_recorder_parser::types::{ObjectClass, ObjectHandle, STARTUP_TASK_NAME, UNNAMED_OBJECT};
use tracing::debug;
use uuid::Uuid;

#[async_trait]
impl TraceRecorderExt<TimelineAttrKey, EventAttrKey> for RecorderData {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error> {
        Ok(ObjectHandle::NO_TASK)
    }

    fn timeline_details(
        &self,
        handle: ContextHandle,
        startup_task_name: Option<&str>,
    ) -> Result<TimelineDetails<TimelineAttrKey>, Error> {
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
                match startup_task_name {
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

        Ok(TimelineDetails {
            name_key: TimelineAttrKey::Common(CommonTimelineAttrKey::Name),
            name,
            description_key: TimelineAttrKey::Common(CommonTimelineAttrKey::Description),
            description,
        })
    }

    async fn setup_common_timeline_attrs(
        &self,
        cfg: &TraceRecorderConfig,
        client: &mut Client<TimelineAttrKey, EventAttrKey>,
    ) -> Result<HashMap<InternedAttrKey, AttrVal>, Error> {
        let mut common_timeline_attr_kvs = HashMap::new();
        let run_id = cfg.plugin.run_id.unwrap_or_else(Uuid::new_v4);
        let time_domain = cfg.plugin.time_domain.unwrap_or_else(Uuid::new_v4);
        debug!(run_id = %run_id);
        for tak in TimelineAttrKey::enumerate() {
            let val = match tak {
                // These are defined by the actual timeline
                TimelineAttrKey::Common(CommonTimelineAttrKey::Name)
                | TimelineAttrKey::Common(CommonTimelineAttrKey::Description) => continue,

                // Only have ns resolution if frequency is non-zero
                TimelineAttrKey::Common(CommonTimelineAttrKey::TimeResolution) => {
                    match self.timestamp_info.timer_frequency.resolution_ns() {
                        None => continue,
                        Some(r) => AttrVal::Timestamp(r),
                    }
                }

                // The rest are common across all timelines
                TimelineAttrKey::Common(CommonTimelineAttrKey::RunId) => run_id.to_string().into(),
                TimelineAttrKey::Common(CommonTimelineAttrKey::TimeDomain) => {
                    time_domain.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Protocol) => {
                    self.protocol.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::KernelVersion) => {
                    self.header.kernel_version.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::KernelPort) => {
                    self.header.kernel_port.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Endianness) => {
                    self.header.endianness.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::IrqPriorityOrder) => {
                    AttrVal::Integer(self.header.irq_priority_order.into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Frequency) => {
                    AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::IsrChainingThreshold) => {
                    AttrVal::Integer(self.header.isr_tail_chaining_threshold.into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Custom(_)) => continue,

                TimelineAttrKey::FormatVersion => {
                    AttrVal::Integer(self.header.format_version.into())
                }
                TimelineAttrKey::NumCores => AttrVal::Integer(self.header.num_cores.into()),
                TimelineAttrKey::PlatformCfg => AttrVal::String(self.header.platform_cfg.clone()),
                TimelineAttrKey::PlatformCfgVersion => {
                    AttrVal::String(self.header.platform_cfg_version.to_string())
                }
                TimelineAttrKey::PlatformCfgVersionMajor => {
                    AttrVal::Integer(self.header.platform_cfg_version.major.into())
                }
                TimelineAttrKey::PlatformCfgVersionMinor => {
                    AttrVal::Integer(self.header.platform_cfg_version.minor.into())
                }
                TimelineAttrKey::PlatformCfgVersionPatch => {
                    AttrVal::Integer(self.header.platform_cfg_version.patch.into())
                }
                TimelineAttrKey::HeapSize => AttrVal::Integer(self.system_heap().max.into()),

                TimelineAttrKey::TimerType => self.timestamp_info.timer_type.to_string().into(),
                TimelineAttrKey::TimerFreq => {
                    AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into())
                }
                TimelineAttrKey::TimerPeriod => {
                    AttrVal::Integer(self.timestamp_info.timer_period.into())
                }
                TimelineAttrKey::TimerWraps => {
                    AttrVal::Integer(self.timestamp_info.timer_wraparounds.into())
                }
                TimelineAttrKey::TickRateHz => {
                    AttrVal::Integer(u32::from(self.timestamp_info.os_tick_rate_hz).into())
                }
                TimelineAttrKey::TickCount => {
                    AttrVal::Integer(self.timestamp_info.os_tick_count.into())
                }
                TimelineAttrKey::LatestTimestampTicks => {
                    BigInt::new_attr_val(self.timestamp_info.latest_timestamp.ticks().into())
                }
                TimelineAttrKey::LatestTimestamp => AttrVal::Timestamp(
                    self.timestamp_info
                        .timer_frequency
                        .lossy_timestamp_ns(self.timestamp_info.latest_timestamp),
                ),
            };
            let key = client.timeline_key(tak.clone()).await?;
            common_timeline_attr_kvs.insert(key, val);
        }

        for kv in cfg
            .ingest
            .timeline_attributes
            .additional_timeline_attributes
            .iter()
        {
            let key = client
                .timeline_key(TimelineAttrKey::Common(CommonTimelineAttrKey::Custom(
                    kv.0.to_string(),
                )))
                .await?;
            common_timeline_attr_kvs.insert(key, kv.1.clone());
        }

        Ok(common_timeline_attr_kvs)
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

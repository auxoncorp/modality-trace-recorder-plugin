use crate::streaming::{EventAttrKey, TimelineAttrKey};
use crate::trace_recorder::Error;
use crate::{
    Client, CommonTimelineAttrKey, ContextHandle, NanosecondsExt, TimelineDetails,
    TraceRecorderConfig, TraceRecorderExt,
};
use async_trait::async_trait;
use derive_more::Display;
use modality_ingest_client::types::{AttrKey, AttrVal};
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
        let obj_class = self.object_data_table.class(obj_handle);
        let obj_name = self
            .symbol_table
            .get(obj_handle)
            .map(|ste| ste.symbol.to_string());
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
    ) -> Result<HashMap<AttrKey, AttrVal>, Error> {
        let mut common_timeline_attr_kvs = HashMap::new();
        let run_id = cfg.rf_opts.run_id.unwrap_or_else(Uuid::new_v4);
        let time_domain = cfg.rf_opts.time_domain.unwrap_or_else(Uuid::new_v4);
        debug!(run_id = %run_id);
        for tak in TimelineAttrKey::enumerate() {
            let key = client.timeline_key(*tak).await?;
            let val = match tak {
                // These are defined by the actual timeline
                TimelineAttrKey::Common(CommonTimelineAttrKey::Name)
                | TimelineAttrKey::Common(CommonTimelineAttrKey::Description) => continue,

                // Only have ns resolution if frequency is non-zero
                TimelineAttrKey::Common(CommonTimelineAttrKey::TimeResolution) => {
                    match self.ts_config_event.frequency.resolution_ns() {
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
                    AttrVal::Integer(u32::from(self.ts_config_event.frequency).into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::IsrChainingThreshold) => {
                    AttrVal::Integer(self.ts_config_event.isr_chaining_threshold.into())
                }

                TimelineAttrKey::FormatVersion => {
                    AttrVal::Integer(self.header.format_version.into())
                }
                TimelineAttrKey::HeapCounter => AttrVal::Integer(self.header.heap_counter.into()),
                TimelineAttrKey::SessionCounter => {
                    AttrVal::Integer(self.start_event.session_counter.into())
                }
                TimelineAttrKey::TickRateHz => {
                    AttrVal::Integer(self.ts_config_event.tick_rate_hz.into())
                }
                TimelineAttrKey::HwTcType => self.ts_config_event.hwtc_type.to_string().into(),
                TimelineAttrKey::HtcPeriod => {
                    if let Some(htc_period) = self.ts_config_event.htc_period {
                        AttrVal::Integer(htc_period.into())
                    } else {
                        continue;
                    }
                }
            };
            common_timeline_attr_kvs.insert(key, val);
        }
        Ok(common_timeline_attr_kvs)
    }
}

fn object_name(name: String, class: MaybeUnknownObjectClass, handle: ObjectHandle) -> String {
    if name.is_empty() {
        format!("{}:{}:{}", UNNAMED_OBJECT, class, handle)
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

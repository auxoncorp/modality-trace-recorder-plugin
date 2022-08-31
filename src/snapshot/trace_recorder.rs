use crate::snapshot::{EventAttrKey, TimelineAttrKey};
use crate::trace_recorder::Error;
use crate::{
    Client, CommonTimelineAttrKey, ContextHandle, NanosecondsExt, TimelineDetails,
    TraceRecorderConfig, TraceRecorderExt,
};
use async_trait::async_trait;
use modality_api::AttrVal;
use modality_ingest_protocol::InternedAttrKey;
use std::collections::HashMap;
use trace_recorder_parser::snapshot::{
    object_properties::{IsrObjectClass, ObjectProperties, TaskObjectClass},
    RecorderData,
};
use trace_recorder_parser::types::{ObjectHandle, STARTUP_TASK_NAME, UNNAMED_OBJECT};
use tracing::debug;
use uuid::Uuid;

#[async_trait]
impl TraceRecorderExt<TimelineAttrKey, EventAttrKey> for RecorderData {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error> {
        self.object_property_table
            .task_object_properties
            .iter()
            .find_map(|(h, p)| {
                if p.name() == Some(STARTUP_TASK_NAME) {
                    Some(*h)
                } else {
                    None
                }
            })
            .ok_or(Error::MissingStartupTaskProperties)
    }

    fn timeline_details(
        &self,
        handle: ContextHandle,
        startup_task_name: Option<&str>,
    ) -> Result<TimelineDetails<TimelineAttrKey>, Error> {
        let (name, description) = match handle {
            ContextHandle::Task(task_handle) => {
                let props = self
                    .object_property_table
                    .task_object_properties
                    .get(&task_handle)
                    .ok_or(Error::TaskPropertiesLookup(task_handle))?;
                let is_startup_task = props.name() == Some(TaskObjectClass::STARTUP_TASK_NAME);
                match startup_task_name {
                    Some(startup_task_name) if is_startup_task => (
                        startup_task_name.to_string(),
                        format!(
                            "{} {} '{}'",
                            self.kernel_port,
                            props.class(),
                            startup_task_name
                        ),
                    ),
                    _ => (
                        timeline_name(task_handle, props),
                        format!(
                            "{} {} '{}'",
                            self.kernel_port,
                            props.class(),
                            props.display_name()
                        ),
                    ),
                }
            }
            ContextHandle::Isr(isr_handle) => {
                let props = self
                    .object_property_table
                    .isr_object_properties
                    .get(&isr_handle)
                    .ok_or(Error::IsrPropertiesLookup(isr_handle))?;
                (
                    isr_name(isr_handle, props),
                    format!(
                        "{} {} '{}'",
                        self.kernel_port,
                        props.class(),
                        props.display_name()
                    ),
                )
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
                    match self.frequency.resolution_ns() {
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
                    self.kernel_version.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::KernelPort) => {
                    self.kernel_port.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Endianness) => {
                    self.endianness.to_string().into()
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::IrqPriorityOrder) => {
                    AttrVal::Integer(self.irq_priority_order.into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Frequency) => {
                    AttrVal::Integer(u32::from(self.frequency).into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::IsrChainingThreshold) => {
                    AttrVal::Integer(self.isr_tail_chaining_threshold.into())
                }
                TimelineAttrKey::Common(CommonTimelineAttrKey::Custom(_)) => continue,

                TimelineAttrKey::MinorVersion => AttrVal::Integer(self.minor_version.into()),
                TimelineAttrKey::FileSize => AttrVal::Integer(self.filesize.into()),
                TimelineAttrKey::NumEvents => AttrVal::Integer(self.num_events.into()),
                TimelineAttrKey::MaxEvents => AttrVal::Integer(self.max_events.into()),
                TimelineAttrKey::BufferFull => self.buffer_is_full.into(),
                TimelineAttrKey::AbsTimeLastEvent => {
                    AttrVal::Integer(self.abs_time_last_event.into())
                }
                TimelineAttrKey::AbsTimeLastEventSecond => {
                    AttrVal::Integer(self.abs_time_last_event_second.into())
                }
                TimelineAttrKey::RecorderActive => self.buffer_is_full.into(),
                TimelineAttrKey::HeapMemUsage => AttrVal::Integer(self.heap_mem_usage.into()),
                TimelineAttrKey::Using16bitHandles => self.is_using_16bit_handles.into(),
                TimelineAttrKey::FloatEncoding => self.float_encoding.to_string().into(),
                TimelineAttrKey::InternalErrorOccured => self.internal_error_occured.into(),
                TimelineAttrKey::SystemInfo => self.system_info.clone().into(),
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

fn timeline_name(handle: ObjectHandle, props: &ObjectProperties<TaskObjectClass>) -> String {
    props
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{}:{}:{}", UNNAMED_OBJECT, props.class(), handle))
}

fn isr_name(handle: ObjectHandle, props: &ObjectProperties<IsrObjectClass>) -> String {
    props
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{}:{}:{}", UNNAMED_OBJECT, props.class(), handle))
}

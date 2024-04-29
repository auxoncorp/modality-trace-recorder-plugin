use crate::{
    attr::TimelineAttrKey, client::Client, config::TraceRecorderConfig, error::Error,
    PLUGIN_VERSION,
};
use async_trait::async_trait;
use auxon_sdk::{
    api::{AttrVal, BigInt, Nanoseconds},
    ingest_protocol::InternedAttrKey,
    reflector_config::AttrKeyEqValuePair,
};
use derive_more::Display;
use std::collections::HashMap;
use trace_recorder_parser::{
    streaming::{
        event::{IsrEvent, TaskEvent},
        RecorderData,
    },
    time::{Frequency, Timestamp},
    types::{ObjectClass, ObjectHandle, STARTUP_TASK_NAME, UNNAMED_OBJECT},
};
use tracing::debug;
use uuid::Uuid;

pub trait NanosecondsExt {
    const ONE_SECOND: u64 = 1_000_000_000;

    fn resolution_ns(&self) -> Option<Nanoseconds>;

    /// Convert to nanosecond time base using the frequency if non-zero,
    /// otherwise fall back to unit ticks
    fn lossy_timestamp_ns<T: Into<Timestamp>>(&self, ticks: T) -> Nanoseconds {
        let t = ticks.into();
        self.resolution_ns()
            .map(|res| Nanoseconds::from(t.get_raw() * res.get_raw()))
            .unwrap_or_else(|| t.get_raw().into())
    }
}

impl NanosecondsExt for Frequency {
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

impl From<TaskEvent> for ContextHandle {
    fn from(event: TaskEvent) -> Self {
        ContextHandle::Task(event.handle)
    }
}

impl From<IsrEvent> for ContextHandle {
    fn from(event: IsrEvent) -> Self {
        ContextHandle::Isr(event.handle)
    }
}

pub struct TimelineDetails {
    pub name_key: TimelineAttrKey,
    pub name: String,
    pub description_key: TimelineAttrKey,
    pub description: String,
    pub object_handle_key: TimelineAttrKey,
    pub object_handle: ObjectHandle,
}

#[async_trait]
pub trait RecorderDataExt {
    fn startup_task_handle(&self) -> Result<ObjectHandle, Error>;

    fn object_handle(&self, obj_name: &str) -> Option<ObjectHandle>;

    fn timeline_details(
        &self,
        handle: ContextHandle,
        startup_task_name: Option<&str>,
    ) -> Result<TimelineDetails, Error>;

    async fn setup_common_timeline_attrs(
        &self,
        cfg: &TraceRecorderConfig,
        client: &mut Client,
    ) -> Result<HashMap<InternedAttrKey, AttrVal>, Error>;
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

    fn timeline_details(
        &self,
        handle: ContextHandle,
        startup_task_name: Option<&str>,
    ) -> Result<TimelineDetails, Error> {
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
            name_key: TimelineAttrKey::Name,
            name,
            description_key: TimelineAttrKey::Description,
            description,
            object_handle_key: TimelineAttrKey::ObjectHandle,
            object_handle: obj_handle,
        })
    }

    async fn setup_common_timeline_attrs(
        &self,
        cfg: &TraceRecorderConfig,
        client: &mut Client,
    ) -> Result<HashMap<InternedAttrKey, AttrVal>, Error> {
        let mut common_timeline_attr_kvs = HashMap::new();
        let run_id = cfg.plugin.run_id.unwrap_or_else(Uuid::new_v4);
        let time_domain = cfg.plugin.time_domain.unwrap_or_else(Uuid::new_v4);
        debug!(run_id = %run_id, time_domain = %time_domain);

        if let Some(r) = self.timestamp_info.timer_frequency.resolution_ns() {
            // Only have ns resolution if frequency is non-zero
            let key = client.timeline_key(TimelineAttrKey::TimeResolution).await?;
            common_timeline_attr_kvs.insert(key, r.into());
        }

        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::RunId).await?,
            run_id.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TimeDomain).await?,
            time_domain.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::ClockStyle).await?,
            "relative".into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::Protocol).await?,
            self.protocol.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::KernelVersion).await?,
            self.header.kernel_version.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::KernelPort).await?,
            self.header.kernel_port.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::Endianness).await?,
            self.header.endianness.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::IrqPriorityOrder)
                .await?,
            AttrVal::Integer(self.header.irq_priority_order.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::Frequency).await?,
            AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::IsrChainingThreshold)
                .await?,
            AttrVal::Integer(self.header.isr_tail_chaining_threshold.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::PluginVersion).await?,
            PLUGIN_VERSION.into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::FormatVersion).await?,
            AttrVal::Integer(self.header.format_version.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::NumCores).await?,
            AttrVal::Integer(self.header.num_cores.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::PlatformCfg).await?,
            AttrVal::String(self.header.platform_cfg.clone().into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::PlatformCfgVersion)
                .await?,
            AttrVal::String(self.header.platform_cfg_version.to_string().into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::PlatformCfgVersionMajor)
                .await?,
            AttrVal::Integer(self.header.platform_cfg_version.major.into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::PlatformCfgVersionMinor)
                .await?,
            AttrVal::Integer(self.header.platform_cfg_version.minor.into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::PlatformCfgVersionPatch)
                .await?,
            AttrVal::Integer(self.header.platform_cfg_version.patch.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::HeapSize).await?,
            AttrVal::Integer(self.system_heap().max.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TimerType).await?,
            self.timestamp_info.timer_type.to_string().into(),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TimerFreq).await?,
            AttrVal::Integer(u32::from(self.timestamp_info.timer_frequency).into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TimerPeriod).await?,
            AttrVal::Integer(self.timestamp_info.timer_period.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TimerWraps).await?,
            AttrVal::Integer(self.timestamp_info.timer_wraparounds.into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TickRateHz).await?,
            AttrVal::Integer(u32::from(self.timestamp_info.os_tick_rate_hz).into()),
        );
        common_timeline_attr_kvs.insert(
            client.timeline_key(TimelineAttrKey::TickCount).await?,
            AttrVal::Integer(self.timestamp_info.os_tick_count.into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::LatestTimestampTicks)
                .await?,
            BigInt::new_attr_val(self.timestamp_info.latest_timestamp.ticks().into()),
        );
        common_timeline_attr_kvs.insert(
            client
                .timeline_key(TimelineAttrKey::LatestTimestamp)
                .await?,
            self.timestamp_info
                .timer_frequency
                .lossy_timestamp_ns(self.timestamp_info.latest_timestamp)
                .into(),
        );

        merge_cfg_attributes(
            &cfg.ingest
                .timeline_attributes
                .additional_timeline_attributes,
            client,
            &mut common_timeline_attr_kvs,
        )
        .await?;

        merge_cfg_attributes(
            &cfg.ingest.timeline_attributes.override_timeline_attributes,
            client,
            &mut common_timeline_attr_kvs,
        )
        .await?;

        Ok(common_timeline_attr_kvs)
    }
}

async fn merge_cfg_attributes(
    attrs_to_merge: &[AttrKeyEqValuePair],
    client: &mut Client,
    attrs: &mut HashMap<InternedAttrKey, AttrVal>,
) -> Result<(), Error> {
    for kv in attrs_to_merge.iter() {
        let fully_qualified_key = format!("timeline.{}", kv.0);
        match client.remove_timeline_string_key(&fully_qualified_key) {
            Some((prev_key, interned_key)) => {
                client.add_timeline_key(prev_key, interned_key);
                attrs.insert(interned_key, kv.1.clone());
            }
            None => {
                let key = client
                    .timeline_key(TimelineAttrKey::Custom(kv.0.to_string()))
                    .await?;
                attrs.insert(key, kv.1.clone());
            }
        }
    }
    Ok(())
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

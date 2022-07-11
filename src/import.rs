use crate::{
    CommonOpts, ContextEvent, ContextHandle, ContextSwitchOutcome, EventAttrKey, EventAttrKeys,
    RenameMap, TimelineAttrKey, TimelineAttrKeys,
};
use modality_ingest_client::{
    types::{AttrKey, AttrVal, BigInt, Nanoseconds, TimelineId},
    BoundTimelineState, IngestClient, IngestClientInitializationError, IngestError, ReadyState,
};
use std::{
    collections::HashMap,
    io::{Read, Seek},
};
use trace_recorder_parser::snapshot::{
    event::user::{Argument, UserEventArgRecordCount},
    event::{Event, EventCode, EventType},
    object_properties::{
        IsrObjectClass, ObjectHandle, ObjectProperties, ObjectPropertyTable, TaskObjectClass,
    },
    Frequency, KernelPortIdentity, RecorderData, Timestamp,
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kernel port {0} is not supported")]
    UnsupportedKernelPortIdentity(KernelPortIdentity),

    #[error(
        "Missing startup task ('{}') object properties",
        TaskObjectClass::STARTUP_TASK_NAME
    )]
    MissingStartupTaskProperties,

    #[error("Failed to locate a timeline for task object handle {0}")]
    TaskTimelineLookup(ObjectHandle),

    #[error("Failed to locate task properties for object handle {0}")]
    TaskPropertiesLookup(ObjectHandle),

    #[error("Failed to locate a timeline for ISR object handle {0}")]
    IsrTimelineLookup(ObjectHandle),

    #[error("Failed to locate ISR properties for object handle {0}")]
    IsrPropertiesLookup(ObjectHandle),

    #[error(
        "User events are only allowed to up to {} events",
        UserEventArgRecordCount::MAX
    )]
    ExceededMaxUserEventArgs,

    #[error(transparent)]
    TraceRecorder(#[from] trace_recorder_parser::snapshot::Error),

    #[error("Encountered an ingest client initialization error. {0}")]
    IngestClientInitialization(#[from] IngestClientInitializationError),

    #[error("Encountered an ingest client error. {0}")]
    Ingest(#[from] IngestError),

    #[error(transparent)]
    Auth(#[from] crate::auth::AuthTokenError),
}

#[derive(Clone, Debug)]
pub struct Config {
    pub common: CommonOpts,
    pub user_event_channel: bool,
    pub user_event_format_string: bool,
    pub user_event_channel_rename_map: RenameMap,
    pub user_event_format_string_rename_map: RenameMap,
    pub single_task_timeline: bool,
    pub flatten_isr_timelines: bool,
    pub startup_task_name: Option<String>,
}

pub async fn import<R: Read + Seek + Send>(mut r: R, cfg: Config) -> Result<(), Error> {
    let trd = RecorderData::locate_and_parse(&mut r)?;

    if trd.kernel_port != KernelPortIdentity::FreeRtos {
        return Err(Error::UnsupportedKernelPortIdentity(trd.kernel_port));
    }

    if trd.num_events == 0 {
        info!("There are no events contained in the RecorderData memory region");
        return Ok(());
    }

    if trd.frequency.is_unitless() {
        warn!("Frequency is zero, time domain will be in unit ticks");
    }

    if trd.num_events > trd.max_events {
        warn!(
            "Event buffer lost {} overwritten records",
            trd.num_events - trd.max_events
        );
    }

    let client = IngestClient::connect(
        &cfg.common.protocol_parent_url,
        cfg.common.allow_insecure_tls,
    )
    .await?;
    let client = client
        .authenticate(cfg.common.resolve_auth()?.into())
        .await?;

    let mut importer = Importer::begin(client, cfg.clone(), &trd).await?;

    for maybe_event in trd.events(&mut r)? {
        let mut attrs = HashMap::new();
        let (event_type, event) = maybe_event?;
        let event_code = EventCode::from(event_type);

        attrs.insert(
            importer.event_key(EventAttrKey::Name).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::EventCode).await?,
            AttrVal::Integer(u8::from(event_code).into()),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::EventType).await?,
            event_type.to_string().into(),
        );

        let timestamp = match event {
            Event::IsrBegin(ev) | Event::IsrResume(ev) => {
                attrs.insert(
                    importer.event_key(EventAttrKey::IsrName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(EventAttrKey::IsrPriority).await?,
                    AttrVal::Integer(u8::from(ev.priority).into()),
                );

                let timestamp = ev.timestamp;

                match importer.context_switch_in(ev.into()).await? {
                    ContextSwitchOutcome::Same => (),
                    ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                        attrs.insert(
                            importer.event_key(EventAttrKey::RemoteTimelineId).await?,
                            AttrVal::TimelineId(Box::new(remote_timeline_id)),
                        );
                        attrs.insert(
                            importer.event_key(EventAttrKey::RemoteTimestamp).await?,
                            AttrVal::Timestamp(trd.frequency.lossy_timestamp_ns(remote_timestamp)),
                        );
                    }
                }

                timestamp
            }

            Event::TaskBegin(ev)
            | Event::TaskReady(ev)
            | Event::TaskResume(ev)
            | Event::TaskCreate(ev) => {
                attrs.insert(
                    importer.event_key(EventAttrKey::TaskName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(EventAttrKey::TaskState).await?,
                    ev.state.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(EventAttrKey::TaskPriority).await?,
                    AttrVal::Integer(u8::from(ev.priority).into()),
                );

                let timestamp = ev.timestamp;

                if matches!(
                    event_type,
                    EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
                ) {
                    match importer.context_switch_in(ev.into()).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            attrs.insert(
                                importer.event_key(EventAttrKey::RemoteTimelineId).await?,
                                AttrVal::TimelineId(Box::new(remote_timeline_id)),
                            );
                            attrs.insert(
                                importer.event_key(EventAttrKey::RemoteTimestamp).await?,
                                AttrVal::Timestamp(
                                    trd.frequency.lossy_timestamp_ns(remote_timestamp),
                                ),
                            );
                        }
                    }
                }

                timestamp
            }

            Event::LowPowerBegin(ev) | Event::LowPowerEnd(ev) => ev.timestamp,

            Event::User(ev) => {
                if cfg.user_event_channel {
                    // Use the channel as the event name
                    attrs.insert(
                        importer.event_key(EventAttrKey::Name).await?,
                        ev.channel.to_string().into(),
                    );
                } else if cfg.user_event_format_string {
                    // Use the format string as the event name
                    attrs.insert(
                        importer.event_key(EventAttrKey::Name).await?,
                        ev.formatted_string.to_string().into(),
                    );
                }

                // Handle channel event name mappings
                if let Some(name) = cfg.user_event_channel_rename_map.get(ev.channel.as_str()) {
                    attrs.insert(
                        importer.event_key(EventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                // Handle format string event name mappings
                if let Some(name) = cfg
                    .user_event_format_string_rename_map
                    .get(ev.formatted_string.as_str())
                {
                    attrs.insert(
                        importer.event_key(EventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                attrs.insert(
                    importer.event_key(EventAttrKey::UserChannel).await?,
                    ev.channel.to_string().into(),
                );
                attrs.insert(
                    importer
                        .event_key(EventAttrKey::UserFormattedString)
                        .await?,
                    ev.formatted_string.to_string().into(),
                );
                for (idx, arg) in ev.args.iter().enumerate() {
                    let key = match idx {
                        0 => importer.event_key(EventAttrKey::UserArg0).await?,
                        1 => importer.event_key(EventAttrKey::UserArg1).await?,
                        2 => importer.event_key(EventAttrKey::UserArg2).await?,
                        3 => importer.event_key(EventAttrKey::UserArg3).await?,
                        4 => importer.event_key(EventAttrKey::UserArg4).await?,
                        5 => importer.event_key(EventAttrKey::UserArg5).await?,
                        6 => importer.event_key(EventAttrKey::UserArg6).await?,
                        7 => importer.event_key(EventAttrKey::UserArg7).await?,
                        8 => importer.event_key(EventAttrKey::UserArg8).await?,
                        9 => importer.event_key(EventAttrKey::UserArg9).await?,
                        10 => importer.event_key(EventAttrKey::UserArg10).await?,
                        11 => importer.event_key(EventAttrKey::UserArg11).await?,
                        12 => importer.event_key(EventAttrKey::UserArg12).await?,
                        13 => importer.event_key(EventAttrKey::UserArg13).await?,
                        14 => importer.event_key(EventAttrKey::UserArg14).await?,
                        _ => return Err(Error::ExceededMaxUserEventArgs),
                    };
                    attrs.insert(key, arg_to_attr_val(arg));
                }
                ev.timestamp
            }

            Event::Unknown(ev) => {
                debug!("Skipping unknown {ev}");
                continue;
            }
        };

        attrs.insert(
            importer.event_key(EventAttrKey::TimestampTicks).await?,
            BigInt::new_attr_val(timestamp.ticks().into()),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::Timestamp).await?,
            AttrVal::Timestamp(trd.frequency.lossy_timestamp_ns(timestamp)),
        );

        importer.event(timestamp, attrs).await?;
    }

    importer.end().await?;

    Ok(())
}

struct Importer {
    single_task_timeline: bool,
    flatten_isr_timelines: bool,
    startup_task_name: Option<String>,

    // Used to track interactions between tasks/ISRs
    startup_task_handle: ObjectHandle,
    handle_of_last_logged_context: ContextHandle,
    timestamp_of_last_event: Timestamp,

    event_keys: EventAttrKeys,
    timeline_keys: TimelineAttrKeys,
    common_timeline_attr_kvs: HashMap<AttrKey, AttrVal>,

    task_to_timeline_ids: HashMap<ObjectHandle, TimelineId>,
    isr_to_timeline_ids: HashMap<ObjectHandle, TimelineId>,

    kernel_port: KernelPortIdentity,
    object_property_table: ObjectPropertyTable,

    ordering: u128,
    client: IngestClient<BoundTimelineState>,
}

impl Importer {
    async fn begin(
        client: IngestClient<ReadyState>,
        cfg: Config,
        trd: &RecorderData,
    ) -> Result<Self, Error> {
        let startup_task_handle = trd
            .object_property_table
            .task_object_properties
            .iter()
            .find(|(_h, p)| p.name() == Some(TaskObjectClass::STARTUP_TASK_NAME))
            .map(|(h, _p)| *h)
            .ok_or(Error::MissingStartupTaskProperties)?;

        // Setup the root startup task timeline
        let startup_task_timeline_id = TimelineId::allocate();
        let mut task_to_timeline_ids = HashMap::new();
        task_to_timeline_ids.insert(startup_task_handle, startup_task_timeline_id);
        let mut client = client.open_timeline(startup_task_timeline_id).await?;

        let mut timeline_keys = TimelineAttrKeys::default();
        let mut common_timeline_attr_kvs = HashMap::new();
        let run_id = cfg.common.run_id.unwrap_or_else(Uuid::new_v4);
        let time_domain = cfg.common.time_domain.unwrap_or_else(Uuid::new_v4);
        for tak in TimelineAttrKey::enumerate() {
            let key = timeline_keys.get(&mut client, *tak).await?;
            let val = match tak {
                // These are defined by the actual timeline
                TimelineAttrKey::Name | TimelineAttrKey::Description => continue,

                // Only have ns resolution if frequency is non-zero
                TimelineAttrKey::TimeResolution => match trd.frequency.resolution_ns() {
                    None => continue,
                    Some(r) => AttrVal::Timestamp(r),
                },

                // The rest are common across all timelines
                TimelineAttrKey::RunId => run_id.to_string().into(),
                TimelineAttrKey::TimeDomain => time_domain.to_string().into(),
                TimelineAttrKey::KernelVersion => trd.kernel_version.to_string().into(),
                TimelineAttrKey::KernelPort => trd.kernel_port.to_string().into(),
                TimelineAttrKey::Endianness => trd.endianness.to_string().into(),
                TimelineAttrKey::MinorVersion => AttrVal::Integer(trd.minor_version.into()),
                TimelineAttrKey::IrqPriorityOrder => {
                    AttrVal::Integer(trd.irq_priority_order.into())
                }
                TimelineAttrKey::FileSize => AttrVal::Integer(trd.filesize.into()),
                TimelineAttrKey::NumEvents => AttrVal::Integer(trd.num_events.into()),
                TimelineAttrKey::MaxEvents => AttrVal::Integer(trd.max_events.into()),
                TimelineAttrKey::BufferFull => trd.buffer_is_full.into(),
                TimelineAttrKey::Frequency => AttrVal::Integer(u32::from(trd.frequency).into()),
                TimelineAttrKey::AbsTimeLastEvent => {
                    AttrVal::Integer(trd.abs_time_last_event.into())
                }
                TimelineAttrKey::AbsTimeLastEventSecond => {
                    AttrVal::Integer(trd.abs_time_last_event_second.into())
                }
                TimelineAttrKey::RecorderActive => trd.buffer_is_full.into(),
                TimelineAttrKey::IsrChainingThreshold => {
                    AttrVal::Integer(trd.isr_tail_chaining_threshold.into())
                }
                TimelineAttrKey::HeapMemUsage => AttrVal::Integer(trd.heap_mem_usage.into()),
                TimelineAttrKey::Using16bitHandles => trd.is_using_16bit_handles.into(),
                TimelineAttrKey::FloatEncoding => trd.float_encoding.to_string().into(),
                TimelineAttrKey::InternalErrorOccured => trd.internal_error_occured.into(),
                TimelineAttrKey::SystemInfo => trd.system_info.clone().into(),
            };
            common_timeline_attr_kvs.insert(key, val);
        }

        let mut importer = Importer {
            single_task_timeline: cfg.single_task_timeline,
            flatten_isr_timelines: cfg.flatten_isr_timelines,
            startup_task_name: cfg.startup_task_name,
            startup_task_handle,
            handle_of_last_logged_context: ContextHandle::Task(startup_task_handle),
            timestamp_of_last_event: Timestamp::zero(),
            event_keys: EventAttrKeys::default(),
            timeline_keys,
            common_timeline_attr_kvs,
            task_to_timeline_ids,
            isr_to_timeline_ids: Default::default(),
            kernel_port: trd.kernel_port,
            object_property_table: trd.object_property_table.clone(),
            ordering: 0,
            client,
        };

        // Add root startup task timeline metadata
        importer
            .add_timeline_metadata(importer.handle_of_last_logged_context)
            .await?;

        Ok(importer)
    }

    async fn end(mut self) -> Result<(), Error> {
        self.client.flush().await?;
        let _ = self.client.close_timeline();
        Ok(())
    }

    async fn event_key(&mut self, key: EventAttrKey) -> Result<AttrKey, Error> {
        let k = self.event_keys.get(&mut self.client, key).await?;
        Ok(k)
    }

    async fn event(
        &mut self,
        timestamp: Timestamp,
        attrs: impl IntoIterator<Item = (AttrKey, AttrVal)>,
    ) -> Result<(), Error> {
        // Keep track of this for interaction remote timestamp on next task switch
        self.timestamp_of_last_event = timestamp;

        // We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        self.client.event(self.ordering, attrs).await?;
        self.ordering += 1;

        Ok(())
    }

    /// Called on the task or ISR context switch-in events
    ///
    /// EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
    /// EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
    async fn context_switch_in(
        &mut self,
        event: ContextEvent,
    ) -> Result<ContextSwitchOutcome, Error> {
        // We've resumed from the same context we were already in and not doing
        // any flattening, nothing to do
        if !self.single_task_timeline
            && !self.flatten_isr_timelines
            && (event.handle() == self.handle_of_last_logged_context)
        {
            return Ok(ContextSwitchOutcome::Same);
        }

        // * a task or ISR context in normal operation
        // * a task context (!single_task_timeline && flatten_isr_timelines)
        // * startup task context or ISR context (single_task_timeline && !flatten_isr_timelines)
        // * always startup task context (single_task_timeline && flatten_isr_timelines)
        let (prev_handle, prev_timeline_id) = match self.handle_of_last_logged_context {
            ContextHandle::Task(h) => (
                ContextHandle::Task(h),
                *self
                    .task_to_timeline_ids
                    .get(&h)
                    .ok_or(Error::TaskTimelineLookup(h))?,
            ),
            ContextHandle::Isr(h) => (
                ContextHandle::Isr(h),
                *self
                    .isr_to_timeline_ids
                    .get(&h)
                    .ok_or(Error::IsrTimelineLookup(h))?,
            ),
        };

        let mut timeline_is_new = false;
        let curr_timeline_id = match &event {
            // EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
            // Resume could be same task or from an ISR, when the task is already marked as active
            ContextEvent::Task(task_event) => {
                let handle = if self.single_task_timeline {
                    self.startup_task_handle
                } else {
                    task_event.handle
                };
                self.handle_of_last_logged_context = ContextHandle::Task(handle);
                *self.task_to_timeline_ids.entry(handle).or_insert_with(|| {
                    timeline_is_new = true;
                    TimelineId::allocate()
                })
            }
            // EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
            // Resume happens when we return to another ISR
            ContextEvent::Isr(isr_event) => {
                if self.flatten_isr_timelines {
                    // Flatten the ISR context into the parent task context
                    self.handle_of_last_logged_context = prev_handle;
                    prev_timeline_id
                } else {
                    self.handle_of_last_logged_context = ContextHandle::Isr(isr_event.handle);
                    *self
                        .isr_to_timeline_ids
                        .entry(isr_event.handle)
                        .or_insert_with(|| {
                            timeline_is_new = true;
                            TimelineId::allocate()
                        })
                }
            }
        };

        self.client.open_timeline(curr_timeline_id).await?;
        if timeline_is_new {
            // Add timeline metadata in the newly updated context
            self.add_timeline_metadata(self.handle_of_last_logged_context)
                .await?;
        }

        if prev_timeline_id != curr_timeline_id {
            Ok(ContextSwitchOutcome::Different(
                prev_timeline_id,
                self.timestamp_of_last_event,
            ))
        } else {
            Ok(ContextSwitchOutcome::Same)
        }
    }

    async fn add_timeline_metadata(&mut self, handle: ContextHandle) -> Result<(), Error> {
        let (timeline_name, timeline_desc) = match handle {
            ContextHandle::Task(handle) => {
                let props = self
                    .object_property_table
                    .task_object_properties
                    .get(&handle)
                    .ok_or(Error::TaskPropertiesLookup(handle))?;

                let is_startup_task = props.name() == Some(TaskObjectClass::STARTUP_TASK_NAME);
                match self.startup_task_name.as_ref() {
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
                        timeline_name(&handle, props),
                        format!(
                            "{} {} '{}'",
                            self.kernel_port,
                            props.class(),
                            props.display_name()
                        ),
                    ),
                }
            }
            ContextHandle::Isr(handle) => {
                let props = self
                    .object_property_table
                    .isr_object_properties
                    .get(&handle)
                    .ok_or(Error::IsrPropertiesLookup(handle))?;
                (
                    isr_name(&handle, props),
                    format!(
                        "{} {} '{}'",
                        self.kernel_port,
                        props.class(),
                        props.display_name()
                    ),
                )
            }
        };

        let mut attr_kvs = self.common_timeline_attr_kvs.clone();
        attr_kvs.insert(
            self.timeline_keys
                .get(&mut self.client, TimelineAttrKey::Name)
                .await?,
            timeline_name.into(),
        );
        attr_kvs.insert(
            self.timeline_keys
                .get(&mut self.client, TimelineAttrKey::Description)
                .await?,
            timeline_desc.into(),
        );

        self.client.timeline_metadata(attr_kvs).await?;

        Ok(())
    }
}

fn timeline_name(handle: &ObjectHandle, props: &ObjectProperties<TaskObjectClass>) -> String {
    props.name().map(|n| n.to_string()).unwrap_or_else(|| {
        format!(
            "{}:{}:{}",
            ObjectProperties::<TaskObjectClass>::UNNAMED_OBJECT,
            props.class(),
            handle
        )
    })
}

fn isr_name(handle: &ObjectHandle, props: &ObjectProperties<IsrObjectClass>) -> String {
    props.name().map(|n| n.to_string()).unwrap_or_else(|| {
        format!(
            "{}:{}:{}",
            ObjectProperties::<IsrObjectClass>::UNNAMED_OBJECT,
            props.class(),
            handle
        )
    })
}

fn arg_to_attr_val(arg: &Argument) -> AttrVal {
    match arg {
        Argument::I8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U8(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U16(v) => AttrVal::Integer(i64::from(*v)),
        Argument::I32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::U32(v) => AttrVal::Integer(i64::from(*v)),
        Argument::F32(v) => AttrVal::Float(v.0.into()),
        Argument::F64(v) => AttrVal::Float(v.0),
        Argument::String(v) => AttrVal::String(v.clone()),
    }
}

pub trait NanosecondsExt {
    const ONE_SECOND: u64 = 1_000_000_000;

    fn resolution_ns(&self) -> Option<Nanoseconds>;

    /// Convert to nanosecond time base using the frequency if non-zero,
    /// otherwise fall back to unit ticks
    fn lossy_timestamp_ns(&self, ticks: Timestamp) -> Nanoseconds {
        self.resolution_ns()
            .map(|res| Nanoseconds::from(ticks.get_raw() * res.get_raw()))
            .unwrap_or_else(|| ticks.get_raw().into())
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

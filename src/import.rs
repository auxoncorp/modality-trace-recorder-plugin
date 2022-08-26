use crate::{
    AttrKeyIndex, Client, CommonEventAttrKey, ContextHandle, ContextSwitchOutcome, NanosecondsExt,
    TraceRecorderConfig, TraceRecorderExt,
};
use modality_ingest_client::{
    types::{AttrVal, BigInt, InternedAttrKey, TimelineId},
    IngestClient, IngestClientInitializationError, IngestError, ReadyState,
};
use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom},
    str::FromStr,
};
use trace_recorder_parser::{
    streaming::HeaderInfo,
    time::{StreamingInstant, Timestamp},
    types::{Argument, KernelPortIdentity, ObjectHandle, UserEventArgRecordCount},
};
use tracing::{debug, error, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kernel port {0} is not supported")]
    UnsupportedKernelPortIdentity(KernelPortIdentity),

    #[error("Failed to locate a timeline for task object handle {0}")]
    TaskTimelineLookup(ObjectHandle),

    #[error("Failed to locate a timeline for ISR object handle {0}")]
    IsrTimelineLookup(ObjectHandle),

    #[error(
        "User events are only allowed to up to {} events",
        UserEventArgRecordCount::MAX
    )]
    ExceededMaxUserEventArgs,

    #[error("The user event format string '{0}' argument count doesn't match the provided custom attribute key set '{1:?}'")]
    FmtArgAttrKeysCountMismatch(String, Vec<String>),

    #[error(transparent)]
    TraceRecorder(#[from] crate::trace_recorder::Error),

    #[error(transparent)]
    SnapshotTraceRecorder(#[from] trace_recorder_parser::snapshot::Error),

    #[error(transparent)]
    StreamingTraceRecorder(#[from] trace_recorder_parser::streaming::Error),

    #[error("Encountered an ingest client initialization error. {0}")]
    IngestClientInitialization(#[from] IngestClientInitializationError),

    #[error("Encountered an ingest client error. {0}")]
    Ingest(#[from] IngestError),

    #[error(transparent)]
    Auth(#[from] crate::auth::AuthTokenError),

    #[error("Encountered an error attemping to parse the protocol parent URL. {0}")]
    Url(#[from] url::ParseError),

    #[error(
        "Encountered and IO error while reading the input stream ({})",
        .0.kind()
    )]
    Io(#[from] io::Error),
}

#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde_with::DeserializeFromStr,
)]
pub enum ImportProtocol {
    Snapshot,
    Streaming,
    Auto,
}

impl Default for ImportProtocol {
    fn default() -> Self {
        ImportProtocol::Auto
    }
}

impl FromStr for ImportProtocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "snapshot" => ImportProtocol::Snapshot,
            "streaming" => ImportProtocol::Streaming,
            "auto" => ImportProtocol::Auto,
            p => return Err(format!("Invalid import protocol '{p}'")),
        })
    }
}

pub async fn import<R: Read + Seek + Send>(
    mut r: R,
    protocol: ImportProtocol,
    cfg: TraceRecorderConfig,
) -> Result<(), Error> {
    match protocol {
        ImportProtocol::Snapshot => import_snapshot(r, cfg).await,
        ImportProtocol::Streaming => import_streaming(r, cfg).await,
        ImportProtocol::Auto => {
            let current_pos = r.stream_position()?;
            debug!("Attempting to detect streaming protocol first");
            let found_psf_word = HeaderInfo::read_psf_word(&mut r).is_ok();
            r.seek(SeekFrom::Start(current_pos))?;
            if found_psf_word {
                import_streaming(r, cfg).await
            } else {
                debug!("Attempting snapshot protocol");
                import_snapshot(r, cfg).await
            }
        }
    }
}

// TODO - factor out the common event attr handling between import_snapshot and import_streaming
// to de-dup things
pub async fn import_snapshot<R: Read + Seek + Send>(
    mut r: R,
    cfg: TraceRecorderConfig,
) -> Result<(), Error> {
    use crate::snapshot::EventAttrKey;
    use trace_recorder_parser::snapshot::{
        event::{Event, EventCode, EventType},
        RecorderData,
    };

    let trd = RecorderData::locate_and_parse(&mut r)?;
    let frequency = trd.frequency;

    if trd.kernel_port != KernelPortIdentity::FreeRtos {
        return Err(Error::UnsupportedKernelPortIdentity(trd.kernel_port));
    }

    if trd.num_events == 0 {
        info!("There are no events contained in the RecorderData memory region");
        return Ok(());
    }

    if frequency.is_unitless() {
        warn!("Frequency is zero, time domain will be in unit ticks");
    }

    if trd.num_events > trd.max_events {
        warn!(
            "Event buffer lost {} overwritten records",
            trd.num_events - trd.max_events
        );
    }

    let client =
        IngestClient::connect(&cfg.protocol_parent_url()?, cfg.ingest.allow_insecure_tls).await?;
    let client = client.authenticate(cfg.resolve_auth()?.into()).await?;

    let mut ordering = 0;
    let mut importer = SnapshotImporter::begin(client, cfg.clone(), &trd).await?;

    for maybe_event in trd.events(&mut r)? {
        let (event_type, event) = maybe_event?;
        let mut attrs = HashMap::new();
        let event_code = EventCode::from(event_type);
        let timestamp = event.timestamp();

        attrs.insert(
            importer.event_key(CommonEventAttrKey::Name).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::EventCode).await?,
            AttrVal::Integer(u8::from(event_code).into()),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::EventType).await?,
            event_type.to_string().into(),
        );

        match event {
            Event::IsrBegin(ev) | Event::IsrResume(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );

                match importer.context_switch_in(ev.into(), &trd).await? {
                    ContextSwitchOutcome::Same => (),
                    ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                        attrs.insert(
                            importer
                                .event_key(CommonEventAttrKey::RemoteTimelineId)
                                .await?,
                            AttrVal::TimelineId(Box::new(remote_timeline_id)),
                        );
                        attrs.insert(
                            importer
                                .event_key(CommonEventAttrKey::RemoteTimestamp)
                                .await?,
                            AttrVal::Timestamp(frequency.lossy_timestamp_ns(remote_timestamp)),
                        );
                    }
                }
            }

            Event::TaskBegin(ev)
            | Event::TaskReady(ev)
            | Event::TaskResume(ev)
            | Event::TaskCreate(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(EventAttrKey::TaskState).await?,
                    ev.state.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
                ) {
                    match importer.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimelineId)
                                    .await?,
                                AttrVal::TimelineId(Box::new(remote_timeline_id)),
                            );
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimestamp)
                                    .await?,
                                AttrVal::Timestamp(frequency.lossy_timestamp_ns(remote_timestamp)),
                            );
                        }
                    }
                }
            }

            Event::User(ev) => {
                if cfg.plugin.user_event_channel {
                    // Use the channel as the event name
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        ev.channel.to_string().into(),
                    );
                } else if cfg.plugin.user_event_format_string {
                    // Use the formatted string as the event name
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        ev.formatted_string.to_string().into(),
                    );
                }

                // Handle channel event name mappings
                if let Some(name) = cfg
                    .plugin
                    .user_event_channel_rename_map
                    .get(ev.channel.as_str())
                {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                // Handle format string event name mappings
                if let Some(name) = cfg
                    .plugin
                    .user_event_formatted_string_rename_map
                    .get(ev.formatted_string.as_str())
                {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                attrs.insert(
                    importer.event_key(CommonEventAttrKey::UserChannel).await?,
                    ev.channel.to_string().into(),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::UserFormattedString)
                        .await?,
                    ev.formatted_string.to_string().into(),
                );

                let custom_arg_keys = cfg
                    .plugin
                    .user_event_fmt_arg_attr_keys
                    .arg_attr_keys(ev.channel.as_str(), &ev.format_string);
                if let Some(custom_arg_keys) = custom_arg_keys {
                    if custom_arg_keys.len() != ev.args.len() {
                        return Err(Error::FmtArgAttrKeysCountMismatch(
                            ev.format_string.into(),
                            custom_arg_keys.to_vec(),
                        ));
                    }
                }

                for (idx, arg) in ev.args.iter().enumerate() {
                    let key = if let Some(custom_arg_keys) = custom_arg_keys {
                        // SAFETY: len checked above
                        importer
                            .event_key(CommonEventAttrKey::CustomUserArg(
                                custom_arg_keys[idx].clone(),
                            ))
                            .await?
                    } else {
                        match idx {
                            0 => importer.event_key(CommonEventAttrKey::UserArg0).await?,
                            1 => importer.event_key(CommonEventAttrKey::UserArg1).await?,
                            2 => importer.event_key(CommonEventAttrKey::UserArg2).await?,
                            3 => importer.event_key(CommonEventAttrKey::UserArg3).await?,
                            4 => importer.event_key(CommonEventAttrKey::UserArg4).await?,
                            5 => importer.event_key(CommonEventAttrKey::UserArg5).await?,
                            6 => importer.event_key(CommonEventAttrKey::UserArg6).await?,
                            7 => importer.event_key(CommonEventAttrKey::UserArg7).await?,
                            8 => importer.event_key(CommonEventAttrKey::UserArg8).await?,
                            9 => importer.event_key(CommonEventAttrKey::UserArg9).await?,
                            10 => importer.event_key(CommonEventAttrKey::UserArg10).await?,
                            11 => importer.event_key(CommonEventAttrKey::UserArg11).await?,
                            12 => importer.event_key(CommonEventAttrKey::UserArg12).await?,
                            13 => importer.event_key(CommonEventAttrKey::UserArg13).await?,
                            14 => importer.event_key(CommonEventAttrKey::UserArg14).await?,
                            _ => return Err(Error::ExceededMaxUserEventArgs),
                        }
                    };
                    attrs.insert(key, arg_to_attr_val(arg));
                }
            }

            // Skip these
            Event::LowPowerBegin(_) | Event::LowPowerEnd(_) => continue,

            Event::Unknown(_ts, ev) => {
                debug!("Skipping unknown {ev}");
                continue;
            }
        }

        attrs.insert(
            importer
                .event_key(CommonEventAttrKey::TimestampTicks)
                .await?,
            BigInt::new_attr_val(timestamp.ticks().into()),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::Timestamp).await?,
            AttrVal::Timestamp(frequency.lossy_timestamp_ns(timestamp)),
        );

        importer.event(timestamp, ordering, attrs).await?;

        // We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        ordering += 1;
    }

    importer.end().await?;

    Ok(())
}

pub async fn import_streaming<R: Read + Send>(
    mut r: R,
    cfg: TraceRecorderConfig,
) -> Result<(), Error> {
    use crate::streaming::EventAttrKey;
    use trace_recorder_parser::streaming::{
        event::{Event, EventType},
        RecorderData,
    };

    let mut trd = RecorderData::read(&mut r)?;
    let frequency = trd.ts_config_event.frequency;

    if trd.header.kernel_port != KernelPortIdentity::FreeRtos {
        return Err(Error::UnsupportedKernelPortIdentity(trd.header.kernel_port));
    }

    if frequency.is_unitless() {
        warn!("Frequency is zero, time domain will be in unit ticks");
    }

    let client =
        IngestClient::connect(&cfg.protocol_parent_url()?, cfg.ingest.allow_insecure_tls).await?;
    let client = client.authenticate(cfg.resolve_auth()?.into()).await?;

    let mut ordering = 0;
    let mut time_rollover_tracker = StreamingInstant::zero();
    let mut importer = StreamingImporter::begin(client, cfg.clone(), &trd).await?;

    // TODO - consider synthesizing the first two events (TraceStart and TsConfig)
    while let Some((event_code, event)) = trd.read_event(&mut r)? {
        let mut attrs = HashMap::new();

        let event_type = event_code.event_type();
        let event_id = event_code.event_id();
        let parameter_count = event_code.parameter_count();

        let event_count = event.event_count();
        let timer_ticks = event.timestamp();
        let timestamp = time_rollover_tracker.elapsed(timer_ticks);

        attrs.insert(
            importer.event_key(CommonEventAttrKey::Name).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::EventCode).await?,
            AttrVal::Integer(u16::from(event_code).into()),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::EventType).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::EventId).await?,
            AttrVal::Integer(u16::from(event_id).into()),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::EventCount).await?,
            AttrVal::Integer(u16::from(event_count).into()),
        );
        attrs.insert(
            importer.event_key(EventAttrKey::ParameterCount).await?,
            AttrVal::Integer(u8::from(parameter_count).into()),
        );

        match event {
            Event::IsrBegin(ev) | Event::IsrResume(ev) | Event::IsrDefine(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
                ) {
                    match importer.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimelineId)
                                    .await?,
                                AttrVal::TimelineId(Box::new(remote_timeline_id)),
                            );
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimestamp)
                                    .await?,
                                AttrVal::Timestamp(frequency.lossy_timestamp_ns(remote_timestamp)),
                            );
                        }
                    }
                }
            }

            Event::TaskBegin(ev)
            | Event::TaskReady(ev)
            | Event::TaskResume(ev)
            | Event::TaskCreate(ev)
            | Event::TaskActivate(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchTaskBegin
                        | EventType::TaskSwitchTaskResume
                        | EventType::TaskActivate
                ) {
                    match importer.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimelineId)
                                    .await?,
                                AttrVal::TimelineId(Box::new(remote_timeline_id)),
                            );
                            attrs.insert(
                                importer
                                    .event_key(CommonEventAttrKey::RemoteTimestamp)
                                    .await?,
                                AttrVal::Timestamp(frequency.lossy_timestamp_ns(remote_timestamp)),
                            );
                        }
                    }
                }
            }

            Event::MemoryAlloc(ev) | Event::MemoryFree(ev) => {
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::MemoryAddress)
                        .await?,
                    AttrVal::Integer(ev.address.into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::MemorySize).await?,
                    AttrVal::Integer(ev.size.into()),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::MemoryHeapCounter)
                        .await?,
                    AttrVal::Integer(ev.heap_counter.into()),
                );
            }

            Event::User(ev) => {
                if cfg.plugin.user_event_channel {
                    // Use the channel as the event name
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        ev.channel.to_string().into(),
                    );
                } else if cfg.plugin.user_event_format_string {
                    // Use the formatted string as the event name
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        ev.formatted_string.to_string().into(),
                    );
                }

                // Handle channel event name mappings
                if let Some(name) = cfg
                    .plugin
                    .user_event_channel_rename_map
                    .get(ev.channel.as_str())
                {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                // Handle format string event name mappings
                if let Some(name) = cfg
                    .plugin
                    .user_event_formatted_string_rename_map
                    .get(ev.formatted_string.as_str())
                {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                attrs.insert(
                    importer.event_key(CommonEventAttrKey::UserChannel).await?,
                    ev.channel.to_string().into(),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::UserFormattedString)
                        .await?,
                    ev.formatted_string.to_string().into(),
                );

                let custom_arg_keys = cfg
                    .plugin
                    .user_event_fmt_arg_attr_keys
                    .arg_attr_keys(ev.channel.as_str(), &ev.format_string);
                if let Some(custom_arg_keys) = custom_arg_keys {
                    if custom_arg_keys.len() != ev.args.len() {
                        return Err(Error::FmtArgAttrKeysCountMismatch(
                            ev.format_string.into(),
                            custom_arg_keys.to_vec(),
                        ));
                    }
                }

                for (idx, arg) in ev.args.iter().enumerate() {
                    let key = if let Some(custom_arg_keys) = custom_arg_keys {
                        // SAFETY: len checked above
                        importer
                            .event_key(CommonEventAttrKey::CustomUserArg(
                                custom_arg_keys[idx].clone(),
                            ))
                            .await?
                    } else {
                        match idx {
                            0 => importer.event_key(CommonEventAttrKey::UserArg0).await?,
                            1 => importer.event_key(CommonEventAttrKey::UserArg1).await?,
                            2 => importer.event_key(CommonEventAttrKey::UserArg2).await?,
                            3 => importer.event_key(CommonEventAttrKey::UserArg3).await?,
                            4 => importer.event_key(CommonEventAttrKey::UserArg4).await?,
                            5 => importer.event_key(CommonEventAttrKey::UserArg5).await?,
                            6 => importer.event_key(CommonEventAttrKey::UserArg6).await?,
                            7 => importer.event_key(CommonEventAttrKey::UserArg7).await?,
                            8 => importer.event_key(CommonEventAttrKey::UserArg8).await?,
                            9 => importer.event_key(CommonEventAttrKey::UserArg9).await?,
                            10 => importer.event_key(CommonEventAttrKey::UserArg10).await?,
                            11 => importer.event_key(CommonEventAttrKey::UserArg11).await?,
                            12 => importer.event_key(CommonEventAttrKey::UserArg12).await?,
                            13 => importer.event_key(CommonEventAttrKey::UserArg13).await?,
                            14 => importer.event_key(CommonEventAttrKey::UserArg14).await?,
                            _ => return Err(Error::ExceededMaxUserEventArgs),
                        }
                    };
                    attrs.insert(key, arg_to_attr_val(arg));
                }
            }

            // Skip These
            Event::ObjectName(_)
            | Event::TraceStart(_)
            | Event::TsConfig(_)
            | Event::TaskPriority(_) => continue,

            Event::Unknown(ev) => {
                debug!("Skipping unknown {ev}");
                continue;
            }
        }

        attrs.insert(
            importer.event_key(EventAttrKey::TimerTicks).await?,
            BigInt::new_attr_val(timer_ticks.ticks().into()),
        );
        attrs.insert(
            importer
                .event_key(CommonEventAttrKey::TimestampTicks)
                .await?,
            BigInt::new_attr_val(timestamp.ticks().into()),
        );
        attrs.insert(
            importer.event_key(CommonEventAttrKey::Timestamp).await?,
            AttrVal::Timestamp(frequency.lossy_timestamp_ns(timestamp)),
        );

        importer.event(timestamp, ordering, attrs).await?;

        // We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        ordering += 1;
    }

    importer.end().await?;

    Ok(())
}

type SnapshotImporter = Importer<crate::snapshot::TimelineAttrKey, crate::snapshot::EventAttrKey>;
type StreamingImporter =
    Importer<crate::streaming::TimelineAttrKey, crate::streaming::EventAttrKey>;

struct Importer<TAK: AttrKeyIndex, EAK: AttrKeyIndex> {
    single_task_timeline: bool,
    flatten_isr_timelines: bool,
    startup_task_name: Option<String>,

    // Used to track interactions between tasks/ISRs
    startup_task_handle: ObjectHandle,
    handle_of_last_logged_context: ContextHandle,
    timestamp_of_last_event: Timestamp,
    task_to_timeline_ids: HashMap<ObjectHandle, TimelineId>,
    isr_to_timeline_ids: HashMap<ObjectHandle, TimelineId>,

    common_timeline_attr_kvs: HashMap<InternedAttrKey, AttrVal>,
    client: Client<TAK, EAK>,
}

impl<TAK: AttrKeyIndex, EAK: AttrKeyIndex> Importer<TAK, EAK> {
    async fn begin<TR: TraceRecorderExt<TAK, EAK>>(
        client: IngestClient<ReadyState>,
        cfg: TraceRecorderConfig,
        trd: &TR,
    ) -> Result<Self, Error> {
        let startup_task_handle = trd.startup_task_handle()?;

        // Setup the root startup task timeline
        let startup_task_timeline_id = TimelineId::allocate();
        let mut task_to_timeline_ids = HashMap::new();
        task_to_timeline_ids.insert(startup_task_handle, startup_task_timeline_id);
        let mut client = Client::new(client.open_timeline(startup_task_timeline_id).await?);

        let common_timeline_attr_kvs = trd.setup_common_timeline_attrs(&cfg, &mut client).await?;

        let mut importer = Importer {
            single_task_timeline: cfg.plugin.single_task_timeline,
            flatten_isr_timelines: cfg.plugin.flatten_isr_timelines,
            startup_task_name: cfg.plugin.startup_task_name,
            startup_task_handle,
            handle_of_last_logged_context: ContextHandle::Task(startup_task_handle),
            timestamp_of_last_event: Timestamp::zero(),
            task_to_timeline_ids,
            isr_to_timeline_ids: Default::default(),
            common_timeline_attr_kvs,
            client,
        };

        // Add root startup task timeline metadata
        importer
            .add_timeline_metadata(importer.handle_of_last_logged_context, trd)
            .await?;

        Ok(importer)
    }

    async fn end(self) -> Result<(), Error> {
        self.client.close().await?;
        Ok(())
    }

    async fn event_key<K: Into<EAK>>(&mut self, key: K) -> Result<InternedAttrKey, Error> {
        let k = self.client.event_key(key).await?;
        Ok(k)
    }

    async fn event(
        &mut self,
        timestamp: Timestamp,
        ordering: u128,
        attrs: impl IntoIterator<Item = (InternedAttrKey, AttrVal)>,
    ) -> Result<(), Error> {
        if timestamp < self.timestamp_of_last_event {
            warn!(
                "Time went backwards from {} to {}",
                self.timestamp_of_last_event, timestamp
            );
        }

        // Keep track of this for interaction remote timestamp on next task switch
        self.timestamp_of_last_event = timestamp;

        self.client.inner().event(ordering, attrs).await?;

        Ok(())
    }

    /// Called on the task or ISR context switch-in events
    ///
    /// EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
    /// EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
    async fn context_switch_in<TR: TraceRecorderExt<TAK, EAK>>(
        &mut self,
        event_context: ContextHandle,
        trd: &TR,
    ) -> Result<ContextSwitchOutcome, Error> {
        // We've resumed from the same context we were already in and not doing
        // any flattening, nothing to do
        if !self.single_task_timeline
            && !self.flatten_isr_timelines
            && (event_context == self.handle_of_last_logged_context)
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
        let curr_timeline_id = match event_context {
            // EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
            // Resume could be same task or from an ISR, when the task is already marked as active
            ContextHandle::Task(task_event_handle) => {
                let handle = if self.single_task_timeline {
                    self.startup_task_handle
                } else {
                    task_event_handle
                };
                self.handle_of_last_logged_context = ContextHandle::Task(handle);
                *self.task_to_timeline_ids.entry(handle).or_insert_with(|| {
                    timeline_is_new = true;
                    TimelineId::allocate()
                })
            }
            // EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
            // Resume happens when we return to another ISR
            ContextHandle::Isr(isr_event_handle) => {
                if self.flatten_isr_timelines {
                    // Flatten the ISR context into the parent task context
                    self.handle_of_last_logged_context = prev_handle;
                    prev_timeline_id
                } else {
                    self.handle_of_last_logged_context = ContextHandle::Isr(isr_event_handle);
                    *self
                        .isr_to_timeline_ids
                        .entry(isr_event_handle)
                        .or_insert_with(|| {
                            timeline_is_new = true;
                            TimelineId::allocate()
                        })
                }
            }
        };

        self.client.inner().open_timeline(curr_timeline_id).await?;
        if timeline_is_new {
            // Add timeline metadata in the newly updated context
            self.add_timeline_metadata(self.handle_of_last_logged_context, trd)
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

    async fn add_timeline_metadata<TR: TraceRecorderExt<TAK, EAK>>(
        &mut self,
        handle: ContextHandle,
        trd: &TR,
    ) -> Result<(), Error> {
        let tl_details = trd.timeline_details(handle, self.startup_task_name.as_deref())?;

        let mut attr_kvs = self.common_timeline_attr_kvs.clone();
        attr_kvs.insert(
            self.client.timeline_key(tl_details.name_key).await?,
            tl_details.name.into(),
        );
        attr_kvs.insert(
            self.client.timeline_key(tl_details.description_key).await?,
            tl_details.description.into(),
        );

        self.client.inner().timeline_metadata(attr_kvs).await?;

        Ok(())
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
        Argument::String(v) => AttrVal::String(v.clone()),
    }
}

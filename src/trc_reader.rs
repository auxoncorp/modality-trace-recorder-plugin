use crate::{
    attr::EventAttrKey,
    config::TraceRecorderConfig,
    context_manager::{arg_to_attr_val, ContextManager, ContextSwitchOutcome},
    deviant_event_parser::DeviantEventParser,
    error::Error,
    interruptor::Interruptor,
    recorder_data::NanosecondsExt,
};
use auxon_sdk::{
    api::{AttrVal, BigInt},
    ingest_client::IngestClient,
};
use std::{collections::HashMap, io::Read};
use trace_recorder_parser::{
    streaming::event::{Event, EventType, TrackingEventCounter},
    streaming::RecorderData,
    time::StreamingInstant,
    types::{KernelPortIdentity, ObjectClass},
};
use tracing::{debug, warn};

pub async fn run<R: Read + Send>(
    mut r: R,
    cfg: TraceRecorderConfig,
    intr: Interruptor,
) -> Result<(), Error> {
    let mut trd = RecorderData::find(&mut r)?;
    let frequency = trd.timestamp_info.timer_frequency;

    if trd.header.kernel_port != KernelPortIdentity::FreeRtos {
        return Err(Error::UnsupportedKernelPortIdentity(trd.header.kernel_port));
    }

    if frequency.is_unitless() {
        warn!("Frequency is zero, time domain will be in unit ticks");
    }

    let mut deviant_event_parser = if let Some(base_event_id) = cfg.plugin.deviant_event_id_base {
        Some(DeviantEventParser::new(
            trd.header.endianness,
            base_event_id.into(),
        )?)
    } else {
        None
    };

    let client =
        IngestClient::connect(&cfg.protocol_parent_url()?, cfg.ingest.allow_insecure_tls).await?;
    let client = client.authenticate(cfg.resolve_auth()?.into()).await?;

    let mut ordering = 0;
    let mut time_rollover_tracker = StreamingInstant::zero();
    let mut event_counter_tracker = TrackingEventCounter::zero();
    let mut first_event_observed = false;
    let mut ctx_mngr = ContextManager::begin(client, cfg.clone(), &trd).await?;

    loop {
        let (event_code, event) = match trd.read_event(&mut r) {
            Ok(Some((ec, ev))) => (ec, ev),
            Ok(None) => break,
            Err(e) => {
                use trace_recorder_parser::streaming::Error as TrcError;
                match e {
                    TrcError::ObjectLookup(_)
                    | TrcError::InvalidObjectHandle(_)
                    | TrcError::FormattedString(_) => {
                        warn!(err=%e, "Downgrading to single timeline mode due to an error in the data");
                        ctx_mngr.set_degraded_single_timeline_mode();
                        continue;
                    }
                    TrcError::TraceRestarted(psf_start_word_endianness) => {
                        warn!("Detected a restarted trace stream");
                        trd =
                            RecorderData::read_with_endianness(psf_start_word_endianness, &mut r)?;
                        first_event_observed = false;
                        continue;
                    }
                    _ => {
                        let _ = ctx_mngr.end().await;
                        return Err(e.into());
                    }
                }
            }
        };
        let mut attrs = HashMap::new();

        let event_type = event_code.event_type();
        let event_id = event_code.event_id();
        let parameter_count = event_code.parameter_count();

        let dropped_events = if !first_event_observed {
            if event_type != EventType::TraceStart {
                warn!(event_type = %event_type, "First event should be TRACE_START");
            }
            event_counter_tracker.set_initial_count(event.event_count());
            first_event_observed = true;
            None
        } else {
            event_counter_tracker.update(event.event_count())
        };

        let event_count_raw = u16::from(event.event_count());
        let event_count = event_counter_tracker.count();
        let timer_ticks = event.timestamp();
        let timestamp = time_rollover_tracker.elapsed(timer_ticks);

        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::Name).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::EventCode).await?,
            AttrVal::Integer(u16::from(event_code).into()),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::EventType).await?,
            event_type.to_string().into(),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::EventId).await?,
            AttrVal::Integer(u16::from(event_id).into()),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::ParameterCount).await?,
            AttrVal::Integer(u8::from(parameter_count).into()),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::EventCountRaw).await?,
            event_count_raw.into(),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::EventCount).await?,
            event_count.into(),
        );
        if let Some(dropped_events) = dropped_events {
            warn!(
                event_count = event_count_raw,
                dropped_events, "Dropped events detected"
            );

            attrs.insert(
                ctx_mngr.event_key(EventAttrKey::DroppedEvents).await?,
                dropped_events.into(),
            );
        }

        match event {
            Event::TraceStart(ev) => {
                // TODO - check if we need to switch to a non-startup task/timeline
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.current_task_handle).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::TaskName).await?,
                    ev.current_task.to_string().into(),
                );
            }

            Event::IsrBegin(ev) | Event::IsrResume(ev) | Event::IsrDefine(ev) => {
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::IsrName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::IsrPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
                ) {
                    match ctx_mngr.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            if !cfg.plugin.disable_task_interactions {
                                attrs.insert(
                                    ctx_mngr.event_key(EventAttrKey::RemoteTimelineId).await?,
                                    AttrVal::TimelineId(Box::new(remote_timeline_id)),
                                );
                                attrs.insert(
                                    ctx_mngr.event_key(EventAttrKey::RemoteTimestamp).await?,
                                    AttrVal::Timestamp(
                                        frequency.lossy_timestamp_ns(remote_timestamp),
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            Event::TaskBegin(ev)
            | Event::TaskReady(ev)
            | Event::TaskResume(ev)
            | Event::TaskCreate(ev)
            | Event::TaskActivate(ev)
            | Event::TaskPriority(ev)
            | Event::TaskPriorityInherit(ev)
            | Event::TaskPriorityDisinherit(ev) => {
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::TaskName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::TaskPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchTaskBegin
                        | EventType::TaskSwitchTaskResume
                        | EventType::TaskActivate
                ) {
                    match ctx_mngr.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            if !cfg.plugin.disable_task_interactions {
                                attrs.insert(
                                    ctx_mngr.event_key(EventAttrKey::RemoteTimelineId).await?,
                                    AttrVal::TimelineId(Box::new(remote_timeline_id)),
                                );
                                attrs.insert(
                                    ctx_mngr.event_key(EventAttrKey::RemoteTimestamp).await?,
                                    AttrVal::Timestamp(
                                        frequency.lossy_timestamp_ns(remote_timestamp),
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            Event::TaskNotify(ev)
            | Event::TaskNotifyFromIsr(ev)
            | Event::TaskNotifyWait(ev)
            | Event::TaskNotifyWaitBlock(ev) => {
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Task)
                {
                    continue;
                }
                if let Some(name) = ev.task_name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::TaskName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::NanosToWait).await?,
                        AttrVal::Timestamp(frequency.lossy_timestamp_ns(ticks_to_wait)),
                    );
                }
            }

            Event::MemoryAlloc(ev) | Event::MemoryFree(ev) => {
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::MemoryAddress).await?,
                    AttrVal::Integer(ev.address.into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::MemorySize).await?,
                    AttrVal::Integer(ev.size.into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::MemoryHeapCurrent).await?,
                    AttrVal::Integer(ev.heap.current.into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::MemoryHeapHighMark).await?,
                    AttrVal::Integer(ev.heap.high_water_mark.into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::MemoryHeapMax).await?,
                    AttrVal::Integer(ev.heap.max.into()),
                );
            }

            Event::UnusedStack(ev) => {
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::TaskName).await?,
                    ev.task.to_string().into(),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::StackLowMark).await?,
                    AttrVal::Integer(ev.low_mark.into()),
                );
            }

            Event::QueueCreate(ev) => {
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Queue)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::QueueName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::QueueLength).await?,
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
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Queue)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::QueueName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    ctx_mngr
                        .event_key(EventAttrKey::QueueMessagesWaiting)
                        .await?,
                    AttrVal::Integer(ev.messages_waiting.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::NanosToWait).await?,
                        AttrVal::Timestamp(frequency.lossy_timestamp_ns(ticks_to_wait)),
                    );
                }
            }

            Event::MutexCreate(ev) => {
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Mutex)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::MutexName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
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
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Mutex)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::MutexName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::NanosToWait).await?,
                        AttrVal::Timestamp(frequency.lossy_timestamp_ns(ticks_to_wait)),
                    );
                }
            }

            Event::SemaphoreBinaryCreate(ev) | Event::SemaphoreCountingCreate(ev) => {
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Semaphore)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::SemaphoreName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(count) = ev.count {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::SemaphoreCount).await?,
                        AttrVal::Integer(count.into()),
                    );
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
                if cfg
                    .plugin
                    .ignored_object_classes
                    .contains(ObjectClass::Semaphore)
                {
                    continue;
                }
                if let Some(name) = ev.name {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::SemaphoreName).await?,
                        AttrVal::String(name.to_string().into()),
                    );
                }
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::SemaphoreCount).await?,
                    AttrVal::Integer(ev.count.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::NanosToWait).await?,
                        AttrVal::Timestamp(frequency.lossy_timestamp_ns(ticks_to_wait)),
                    );
                }
            }

            Event::User(ev) => {
                ctx_mngr.handle_device_timeline_id_channel_event(
                    &ev.channel,
                    &ev.format_string,
                    &ev.formatted_string,
                    &ev.args,
                    &trd,
                );

                if cfg.plugin.user_event_channel {
                    // Use the channel as the event name
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
                        ev.channel.to_string().into(),
                    );
                } else if cfg.plugin.user_event_format_string {
                    // Use the formatted string as the event name
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
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
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                } else if ev.channel.as_str() == "#WFR" {
                    warn!(
                        msg = %ev.formatted_string,
                        "Target produced a warning on the '#WFR' channel"
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
                        "WARNING_FROM_RECORDER".into(),
                    );
                }

                // Handle format string event name mappings
                if let Some(name) = cfg
                    .plugin
                    .user_event_formatted_string_rename_map
                    .get(ev.formatted_string.as_str())
                {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
                        name.to_string().into(),
                    );
                }

                attrs.insert(
                    ctx_mngr.event_key(EventAttrKey::UserChannel).await?,
                    ev.channel.to_string().into(),
                );
                attrs.insert(
                    ctx_mngr
                        .event_key(EventAttrKey::UserFormattedString)
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
                        ctx_mngr
                            .event_key(EventAttrKey::CustomUserArg(custom_arg_keys[idx].clone()))
                            .await?
                    } else {
                        match idx {
                            0 => ctx_mngr.event_key(EventAttrKey::UserArg0).await?,
                            1 => ctx_mngr.event_key(EventAttrKey::UserArg1).await?,
                            2 => ctx_mngr.event_key(EventAttrKey::UserArg2).await?,
                            3 => ctx_mngr.event_key(EventAttrKey::UserArg3).await?,
                            4 => ctx_mngr.event_key(EventAttrKey::UserArg4).await?,
                            5 => ctx_mngr.event_key(EventAttrKey::UserArg5).await?,
                            6 => ctx_mngr.event_key(EventAttrKey::UserArg6).await?,
                            7 => ctx_mngr.event_key(EventAttrKey::UserArg7).await?,
                            8 => ctx_mngr.event_key(EventAttrKey::UserArg8).await?,
                            9 => ctx_mngr.event_key(EventAttrKey::UserArg9).await?,
                            10 => ctx_mngr.event_key(EventAttrKey::UserArg10).await?,
                            11 => ctx_mngr.event_key(EventAttrKey::UserArg11).await?,
                            12 => ctx_mngr.event_key(EventAttrKey::UserArg12).await?,
                            13 => ctx_mngr.event_key(EventAttrKey::UserArg13).await?,
                            14 => ctx_mngr.event_key(EventAttrKey::UserArg14).await?,
                            _ => return Err(Error::ExceededMaxUserEventArgs),
                        }
                    };
                    attrs.insert(key, arg_to_attr_val(arg));
                }
            }

            // Skip These
            Event::ObjectName(_) | Event::TsConfig(_) => continue,

            Event::Unknown(ev) => {
                let maybe_dev = if let Some(p) = deviant_event_parser.as_mut() {
                    p.parse(&ev)?
                } else {
                    None
                };

                if let Some(dev) = maybe_dev {
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::Name).await?,
                        dev.kind.to_modality_name().into(),
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::MutatorId).await?,
                        dev.mutator_id.1,
                    );
                    attrs.insert(
                        ctx_mngr.event_key(EventAttrKey::InternalMutatorId).await?,
                        dev.mutator_id.0.to_string().into(),
                    );
                    if let Some(m) = dev.mutation_id {
                        attrs.insert(ctx_mngr.event_key(EventAttrKey::MutationId).await?, m.1);
                        attrs.insert(
                            ctx_mngr.event_key(EventAttrKey::InternalMutationId).await?,
                            m.0.to_string().into(),
                        );
                    }
                    if let Some(s) = dev.mutation_success {
                        attrs.insert(ctx_mngr.event_key(EventAttrKey::MutationSuccess).await?, s);
                    }
                } else if !cfg.plugin.include_unknown_events {
                    debug!(
                        %event_type,
                        timestamp = %ev.timestamp,
                        id = %ev.code.event_id(),
                        event_count = %ev.event_count, "Skipping unknown");
                    continue;
                }
            }
        }

        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::TimerTicks).await?,
            BigInt::new_attr_val(timer_ticks.ticks().into()),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::TimestampTicks).await?,
            BigInt::new_attr_val(timestamp.ticks().into()),
        );
        attrs.insert(
            ctx_mngr.event_key(EventAttrKey::Timestamp).await?,
            AttrVal::Timestamp(frequency.lossy_timestamp_ns(timestamp)),
        );

        ctx_mngr.event(timestamp, ordering, attrs).await?;

        // We get events in logical (and tomporal) order, so only need
        // a local counter for ordering
        ordering += 1;

        if intr.is_set() {
            break;
        }
    }

    ctx_mngr.end().await?;

    Ok(())
}

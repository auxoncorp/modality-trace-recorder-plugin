use crate::import::{arg_to_attr_val, Error, StreamingImporter};
use crate::{CommonEventAttrKey, ContextSwitchOutcome, NanosecondsExt, TraceRecorderConfig};
use modality_api::{AttrVal, BigInt};
use modality_ingest_client::IngestClient;
use std::{collections::HashMap, io::Read};
use trace_recorder_parser::{
    time::StreamingInstant,
    types::{KernelPortIdentity, ObjectClass},
};
use tracing::{debug, warn};

// TODO - factor out the common event attr handling between import_snapshot and import_streaming
// to de-dup things
pub async fn import<R: Read + Send>(mut r: R, cfg: TraceRecorderConfig) -> Result<(), Error> {
    use crate::streaming::EventAttrKey;
    use trace_recorder_parser::streaming::{
        event::{Event, EventType},
        RecorderData,
    };

    let mut trd = RecorderData::read(&mut r)?;
    let frequency = trd.timestamp_info.timer_frequency;

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
            Event::TraceStart(ev) => {
                // TODO - check if we need to switch to a non-startup task/timeline
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.current_task_handle).into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskName).await?,
                    ev.current_task.to_string().into(),
                );
            }

            Event::IsrBegin(ev) | Event::IsrResume(ev) | Event::IsrDefine(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::IsrPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchIsrBegin | EventType::TaskSwitchIsrResume
                ) {
                    match importer.context_switch_in(ev.into(), &trd).await? {
                        ContextSwitchOutcome::Same => (),
                        ContextSwitchOutcome::Different(remote_timeline_id, remote_timestamp) => {
                            if !cfg.plugin.disable_task_interactions {
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
                    importer.event_key(CommonEventAttrKey::TaskName).await?,
                    ev.name.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskPriority).await?,
                    AttrVal::Integer(u32::from(ev.priority).into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
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
                            if !cfg.plugin.disable_task_interactions {
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
                                    AttrVal::Timestamp(
                                        frequency.lossy_timestamp_ns(remote_timestamp),
                                    ),
                                );
                            }
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
                        .event_key(CommonEventAttrKey::MemoryHeapCurrent)
                        .await?,
                    AttrVal::Integer(ev.heap.current.into()),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::MemoryHeapHighMark)
                        .await?,
                    AttrVal::Integer(ev.heap.high_water_mark.into()),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::MemoryHeapMax)
                        .await?,
                    AttrVal::Integer(ev.heap.max.into()),
                );
            }

            Event::UnusedStack(ev) => {
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::TaskName).await?,
                    ev.task.to_string().into(),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::StackLowMark).await?,
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
                        importer.event_key(CommonEventAttrKey::QueueName).await?,
                        AttrVal::String(name.into()),
                    );
                }
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::QueueLength).await?,
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
                        importer.event_key(CommonEventAttrKey::QueueName).await?,
                        AttrVal::String(name.into()),
                    );
                }
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::QueueMessagesWaiting)
                        .await?,
                    AttrVal::Integer(ev.messages_waiting.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::NanosToWait).await?,
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
                        importer
                            .event_key(CommonEventAttrKey::SemaphoreName)
                            .await?,
                        AttrVal::String(name.into()),
                    );
                }
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                if let Some(count) = ev.count {
                    attrs.insert(
                        importer
                            .event_key(CommonEventAttrKey::SemaphoreCount)
                            .await?,
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
                        importer
                            .event_key(CommonEventAttrKey::SemaphoreName)
                            .await?,
                        AttrVal::String(name.into()),
                    );
                }
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );
                attrs.insert(
                    importer
                        .event_key(CommonEventAttrKey::SemaphoreCount)
                        .await?,
                    AttrVal::Integer(ev.count.into()),
                );
                if let Some(ticks_to_wait) = ev.ticks_to_wait {
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::TicksToWait).await?,
                        AttrVal::Integer(u32::from(ticks_to_wait).into()),
                    );
                    attrs.insert(
                        importer.event_key(CommonEventAttrKey::NanosToWait).await?,
                        AttrVal::Timestamp(frequency.lossy_timestamp_ns(ticks_to_wait)),
                    );
                }
            }

            Event::User(ev) => {
                importer.handle_device_timeline_id_channel_event(
                    &ev.channel,
                    &ev.format_string,
                    &ev.formatted_string,
                    &ev.args,
                    &trd,
                );

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
            Event::ObjectName(_) | Event::TsConfig(_) => continue,

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

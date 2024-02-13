use crate::import::{arg_to_attr_val, Error, SnapshotImporter};
use crate::{CommonEventAttrKey, ContextSwitchOutcome, NanosecondsExt, TraceRecorderConfig};
use modality_api::{AttrVal, BigInt};
use modality_ingest_client::IngestClient;
use std::{
    collections::HashMap,
    io::{Read, Seek},
};
use trace_recorder_parser::types::KernelPortIdentity;
use tracing::{debug, info, warn};

// TODO - factor out the common event attr handling between import_snapshot and import_streaming
// to de-dup things
pub async fn import<R: Read + Seek + Send>(
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

    if cfg.plugin.use_timeline_id_channel {
        warn!("Configuration field 'use-timeline-id-channel` is not supported in snapshot mode");
    }
    if cfg.plugin.deviant_event_id_base.is_some() {
        warn!("Configuration field 'deviant-event-id-base` is not supported in snapshot mode");
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
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );

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
                                AttrVal::Timestamp(frequency.lossy_timestamp_ns(remote_timestamp)),
                            );
                        }
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
                attrs.insert(
                    importer.event_key(CommonEventAttrKey::ObjectHandle).await?,
                    AttrVal::Integer(u32::from(ev.handle).into()),
                );

                if matches!(
                    event_type,
                    EventType::TaskSwitchTaskBegin | EventType::TaskSwitchTaskResume
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

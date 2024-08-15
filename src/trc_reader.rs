use crate::{
    client::Client,
    config::TraceRecorderConfig,
    context_manager::{ContextEvent, ContextManager},
    error::Error,
    interruptor::Interruptor,
    recorder_data::TimelineAttributes,
};
use auxon_sdk::ingest_client::IngestClient;
use std::{collections::HashSet, io::Read};
use trace_recorder_parser::{streaming::RecorderData, types::KernelPortIdentity};
use tracing::{debug, trace, warn};

pub async fn run<R: Read + Send>(
    mut r: R,
    cfg: TraceRecorderConfig,
    intr: Interruptor,
) -> Result<(), Error> {
    let mut trd = RecorderData::find(&mut r)?;

    let maybe_custom_printf_event_id = cfg.plugin.custom_printf_event_id;
    if let Some(custom_printf_event_id) = maybe_custom_printf_event_id {
        debug!(custom_printf_event_id, "Setting custom printf event ID");
        trd.set_custom_printf_event_id(custom_printf_event_id.into());
    }

    let frequency = trd.timestamp_info.timer_frequency;

    if trd.header.kernel_port != KernelPortIdentity::FreeRtos {
        return Err(Error::UnsupportedKernelPortIdentity(trd.header.kernel_port));
    }

    if frequency.is_unitless() {
        warn!("Frequency is zero, time domain will be in unit ticks");
    }

    let client =
        IngestClient::connect(&cfg.protocol_parent_url()?, cfg.ingest.allow_insecure_tls).await?;
    let mut client = Client::new(client.authenticate(cfg.resolve_auth()?.into()).await?);
    let mut ctx_mngr = ContextManager::new(cfg, &trd)?;
    let mut observed_timelines = HashSet::new();
    let mut buffered_event: Option<ContextEvent> = None;
    let mut maybe_result: Option<Result<(), Error>> = None;

    while !intr.is_set() {
        let (event_code, event) = match trd.read_event(&mut r) {
            Ok(Some((ec, ev))) => (ec, ev),
            Ok(None) => break,
            Err(e) => {
                use trace_recorder_parser::streaming::Error as TrcError;
                match e {
                    TrcError::ObjectLookup(_) | TrcError::InvalidObjectHandle(_) => {
                        ctx_mngr.set_degraded(format!("Data error {e}"));
                        continue;
                    }
                    TrcError::FixedUserEventFmtStringLookup(_) | TrcError::FormattedString(_) => {
                        warn!("Encountered an invalid user event. {e}");
                        continue;
                    }
                    TrcError::TraceRestarted(psf_start_word_endianness) => {
                        warn!("Detected a restarted trace stream");
                        trd =
                            RecorderData::read_with_endianness(psf_start_word_endianness, &mut r)?;
                        if let Some(custom_printf_event_id) = maybe_custom_printf_event_id {
                            trd.set_custom_printf_event_id(custom_printf_event_id.into());
                        }
                        ctx_mngr.observe_trace_restart();
                        continue;
                    }
                    _ => {
                        // Store the result so we can pass it along after flushing buffered events
                        maybe_result = Some(Err(e.into()));
                        break;
                    }
                }
            }
        };

        trace!(%event_code, %event, "Received event");

        let ctx_event = match ctx_mngr.process_event(event_code, &event, &trd) {
            Ok(Some(ev)) => ev,
            Ok(None) => continue,
            Err(e) => {
                // Store the result so we can pass it along after flushing buffered events
                maybe_result = Some(Err(e));
                break;
            }
        };

        // Maintain a 1-element buffer so we can ensure the interaction nonce attr key
        // is present on the previous event when we encounter a context switch
        // on the current event
        match buffered_event.take() {
            Some(mut prev_event) => {
                if ctx_event.add_previous_event_nonce {
                    prev_event.promote_internal_nonce();
                }

                // Buffer the current event
                buffered_event = Some(ctx_event);

                // Send the previous event
                let timeline = ctx_mngr.timeline_meta(prev_event.context)?;
                let mut new_timeline_attrs: Option<&TimelineAttributes> = None;
                if observed_timelines.insert(timeline.id()) {
                    new_timeline_attrs = Some(timeline.attributes());
                }

                client
                    .switch_timeline(timeline.id(), new_timeline_attrs)
                    .await?;

                client
                    .send_event(prev_event.global_ordering, prev_event.attributes.iter())
                    .await?;
            }

            // First iter of the loop
            None => {
                buffered_event = Some(ctx_event);
            }
        }
    }

    // Flush the last event
    if let Some(last_event) = buffered_event.take() {
        debug!("Flushing buffered events");
        let timeline = ctx_mngr.timeline_meta(last_event.context)?;
        let mut new_timeline_attrs: Option<&TimelineAttributes> = None;
        if observed_timelines.insert(timeline.id()) {
            new_timeline_attrs = Some(timeline.attributes());
        }

        client
            .switch_timeline(timeline.id(), new_timeline_attrs)
            .await?;

        client
            .send_event(last_event.global_ordering, last_event.attributes.iter())
            .await?;
    }

    client.inner.flush().await?;

    if let Ok(status) = client.inner.status().await {
        debug!(
            events_received = status.events_received,
            events_written = status.events_written,
            events_pending = status.events_pending,
            "Ingest status"
        );
    }

    if let Some(res) = maybe_result {
        res
    } else {
        Ok(())
    }
}

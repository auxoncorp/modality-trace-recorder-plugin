use crate::trace_recorder::Error;
use byteordered::ByteOrdered;
use modality_api::AttrVal;
use std::io::Read;
use trace_recorder_parser::{
    streaming::event::{BaseEvent, EventId},
    types::Endianness,
};
use uuid::Uuid;

#[derive(Debug)]
pub struct DeviantEventParser {
    endianness: Endianness,
    base_event_id: EventId,
    uuid_bytes: [u8; 16],
    scratch: Vec<u8>,
}

impl DeviantEventParser {
    // Event ID offsets from the base
    const MUTATOR_ANNOUNCED_OFFSET: u16 = 0;
    const MUTATOR_RETIRED_OFFSET: u16 = 1;
    const MUTATION_CMD_COMM_OFFSET: u16 = 2;
    const MUTATION_CLR_COMM_OFFSET: u16 = 3;
    const MUTATION_TRIGGERED_OFFSET: u16 = 4;
    const MUTATION_INJECTED_OFFSET: u16 = 5;

    pub fn new(endianness: Endianness, base_event_id: EventId) -> Result<Self, Error> {
        if base_event_id.0 > 0x0F_FF {
            Err(Error::DeviantEvent(format!(
                "The deviant custom event base ID {} exceeds the max",
                base_event_id
            )))
        } else {
            Ok(Self {
                endianness,
                base_event_id,
                uuid_bytes: [0; 16],
                scratch: Vec::with_capacity(64),
            })
        }
    }

    /// Returns true if the event was a deviant custom event
    pub fn parse(&mut self, event: &BaseEvent) -> Result<Option<DeviantEvent>, Error> {
        let kind = match DeviantEventKind::from_base_event(self.base_event_id, event) {
            Some(k) => k,
            None => return Ok(None),
        };
        let params = event.parameters();
        self.scratch.clear();
        for p in params {
            self.scratch.extend_from_slice(&p.to_le_bytes());
        }

        if self.scratch.len() != kind.expected_parameter_byte_count() {
            return Err(Error::DeviantEvent(format!(
                "The event {} ({}) has an incorrect number of parameter bytes ({}), expected {}",
                event.code,
                kind.to_modality_name(),
                self.scratch.len(),
                kind.expected_parameter_byte_count(),
            )));
        }

        let mut r = ByteOrdered::new(
            self.scratch.as_slice(),
            byteordered::Endianness::from(self.endianness),
        );

        r.read_exact(&mut self.uuid_bytes)?;
        let mutator_uuid = Uuid::from_bytes(self.uuid_bytes);

        let mut deviant_event = DeviantEvent {
            kind,
            mutator_id: (mutator_uuid, uuid_to_integer_attr_val(&mutator_uuid)),
            mutation_id: None,
            mutation_success: None,
        };

        match kind {
            DeviantEventKind::MutatorAnnounced | DeviantEventKind::MutatorRetired => (),
            _ => {
                r.read_exact(&mut self.uuid_bytes)?;
                let mutation_uuid = Uuid::from_bytes(self.uuid_bytes);
                let mutation_success = r.read_u32()?;
                deviant_event.mutation_id =
                    Some((mutation_uuid, uuid_to_integer_attr_val(&mutation_uuid)));
                deviant_event.mutation_success = Some((mutation_success != 0).into());
            }
        }

        Ok(Some(deviant_event))
    }
}

#[derive(Clone, Debug)]
pub struct DeviantEvent {
    pub kind: DeviantEventKind,
    pub mutator_id: (Uuid, AttrVal),
    pub mutation_id: Option<(Uuid, AttrVal)>,
    pub mutation_success: Option<AttrVal>,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum DeviantEventKind {
    MutatorAnnounced,
    MutatorRetired,
    MutationCmdCommunicated,
    MutationClearCommunicated,
    MutationTriggered,
    MutationInjected,
}

impl DeviantEventKind {
    pub const fn to_modality_name(self) -> &'static str {
        use DeviantEventKind::*;
        match self {
            MutatorAnnounced => "modality.mutator.announced",
            MutatorRetired => "modality.mutator.retired",
            MutationCmdCommunicated => "modality.mutation.command_communicated",
            MutationClearCommunicated => "modality.mutation.clear_communicated",
            MutationTriggered => "modality.mutation.triggered",
            MutationInjected => "modality.mutation.injected",
        }
    }

    const fn expected_parameter_byte_count(self) -> usize {
        use DeviantEventKind::*;
        match self {
            MutatorAnnounced | MutatorRetired => 16, // UUID
            _ => 16 + 16 + 4,                        // UUID, UUID, u32
        }
    }

    fn from_base_event(base_event_id: EventId, event: &BaseEvent) -> Option<Self> {
        use DeviantEventKind::*;

        if event.code.event_id().0 >= base_event_id.0 {
            let offset = event.code.event_id().0 - base_event_id.0;
            Some(match offset {
                DeviantEventParser::MUTATOR_ANNOUNCED_OFFSET => MutatorAnnounced,
                DeviantEventParser::MUTATOR_RETIRED_OFFSET => MutatorRetired,
                DeviantEventParser::MUTATION_CMD_COMM_OFFSET => MutationCmdCommunicated,
                DeviantEventParser::MUTATION_CLR_COMM_OFFSET => MutationClearCommunicated,
                DeviantEventParser::MUTATION_TRIGGERED_OFFSET => MutationTriggered,
                DeviantEventParser::MUTATION_INJECTED_OFFSET => MutationInjected,
                _ => return None,
            })
        } else {
            None
        }
    }
}

fn uuid_to_integer_attr_val(u: &Uuid) -> AttrVal {
    i128::from_le_bytes(*u.as_bytes()).into()
}

use derive_more::Display;

/// Trace recorder control-plane commands
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display)]
pub enum Command {
    /// CMD_SET_ACTIVE with param1 set to 1, start tracing
    #[display(fmt = "Start")]
    Start,
    /// CMD_SET_ACTIVE with param1 set to 0, stop tracing
    #[display(fmt = "Stop")]
    Stop,
}

impl Command {
    /// Wire size of a command, equivalent to `sizeof(TracealyzerCommandType)`
    pub const WIRE_SIZE: usize = 8;

    /// param1 = 1 means start, param1 = 0 means stop
    const CMD_SET_ACTIVE: u8 = 1;

    fn param1(&self) -> u8 {
        match self {
            Command::Start => 1,
            Command::Stop => 0,
        }
    }

    fn checksum(&self) -> u16 {
        let sum: u16 = u16::from(Self::CMD_SET_ACTIVE)
              // param2..=param5 are always zero
              + u16::from(self.param1());
        0xFFFF_u16.wrapping_sub(sum)
    }

    /// Convert a command to its `TracealyzerCommandType` wire representation
    pub fn to_le_bytes(&self) -> [u8; Self::WIRE_SIZE] {
        let checksum_bytes = self.checksum().to_le_bytes();
        [
            Self::CMD_SET_ACTIVE,
            self.param1(),
            0,
            0,
            0,
            0,
            checksum_bytes[0],
            checksum_bytes[1],
        ]
    }
}

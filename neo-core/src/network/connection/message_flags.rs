use serde::{Serialize, Deserialize};

/// Represents the flags of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageFlags {
    /// No flag is set for the message.
    None = 0,
    /// Indicates that the message is compressed.
    Compressed = 1,
}

impl MessageFlags {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(MessageFlags::None),
            1 => Some(MessageFlags::Compressed),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

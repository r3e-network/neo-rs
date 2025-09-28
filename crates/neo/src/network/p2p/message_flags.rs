//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use crate::NetworkError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Message flags applied to the network payload header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageFlags {
    /// No flags are set.
    None = 0x00,
    /// The payload is compressed.
    Compressed = 0x01,
}

impl MessageFlags {
    /// Converts the flags to their byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Alias for [`to_byte`]; retained for backward compatibility.
    pub fn as_byte(self) -> u8 {
        self.to_byte()
    }

    /// Parses the flags from their byte representation.
    pub fn from_byte(byte: u8) -> Result<Self, NetworkError> {
        match byte {
            0x00 => Ok(Self::None),
            0x01 => Ok(Self::Compressed),
            other => Err(NetworkError::ProtocolViolation {
                peer: SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown message flags: 0x{:02x}", other),
            }),
        }
    }

    /// Returns `true` when the compressed flag is set.
    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Compressed)
    }
}

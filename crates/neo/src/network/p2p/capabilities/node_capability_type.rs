//! Capability type identifiers (mirrors `NodeCapabilityType.cs`).

use serde::{Deserialize, Serialize};

/// Enumerates the well-known capability identifiers while preserving
/// extensibility for custom or future types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeCapabilityType {
    /// Indicates that the node is listening on a TCP port.
    TcpServer,
    /// Indicates that the node is listening on a WebSocket port.
    WsServer,
    /// Disables peer-to-peer compression for the advertising node.
    DisableCompression,
    /// Indicates that the node keeps the full current state.
    FullNode,
    /// Indicates that the node stores full block history.
    ArchivalNode,
    /// Reserved extension identifier (0xF0) for private capabilities.
    Extension0,
    /// Any identifier that is not currently recognised by the Rust port.
    Unknown(u8),
}

impl NodeCapabilityType {
    /// Maximum encoded value accepted by the Neo protocol for capability bytes.
    pub const MAX_VALUE: u8 = u8::MAX;

    /// Creates a capability type from its byte representation.
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x01 => Self::TcpServer,
            0x02 => Self::WsServer,
            0x03 => Self::DisableCompression,
            0x10 => Self::FullNode,
            0x11 => Self::ArchivalNode,
            0xF0 => Self::Extension0,
            other => Self::Unknown(other),
        }
    }

    /// Returns the byte representation expected on the wire.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::TcpServer => 0x01,
            Self::WsServer => 0x02,
            Self::DisableCompression => 0x03,
            Self::FullNode => 0x10,
            Self::ArchivalNode => 0x11,
            Self::Extension0 => 0xF0,
            Self::Unknown(value) => value,
        }
    }

    /// Returns `true` when the capability is one of the well-known variants.
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

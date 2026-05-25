//! Capability type identifiers (mirrors `NodeCapabilityType.cs`).

use neo_primitives::protocol_enum_with_unknown;
use serde::{Deserialize, Serialize};

protocol_enum_with_unknown! {
    /// Enumerates the well-known capability identifiers while preserving
    /// extensibility for custom or future types.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub NodeCapabilityType {
        unknown
        /// Any identifier that is not currently recognised by the Rust port.
        Unknown(u8) => "Unknown";

        /// Indicates that the node is listening on a TCP port.
        TcpServer = 0x01 => "TcpServer",
        /// Indicates that the node is listening on a WebSocket port.
        WsServer = 0x02 => "WsServer",
        /// Disables peer-to-peer compression for the advertising node.
        DisableCompression = 0x03 => "DisableCompression",
        /// Indicates that the node keeps the full current state.
        FullNode = 0x10 => "FullNode",
        /// Indicates that the node stores full block history.
        ArchivalNode = 0x11 => "ArchivalNode",
        /// Reserved extension identifier (0xF0) for private capabilities.
        Extension0 = 0xF0 => "Extension0",
    }
}

impl NodeCapabilityType {
    /// Maximum encoded value accepted by the Neo protocol for capability bytes.
    pub const MAX_VALUE: u8 = u8::MAX;
}

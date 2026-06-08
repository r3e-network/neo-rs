//! Node capability type identifiers.
//!
//! Mirrors `Neo.Network.P2P.Capabilities.NodeCapabilityType`.

use crate::protocol_enum_with_unknown;
use serde::{Deserialize, Serialize};

protocol_enum_with_unknown! {
    /// Enumerates the well-known capability identifiers while preserving
    /// extensibility for custom or future types.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub NodeCapabilityType {
        unknown
        /// Any identifier that is not currently recognised by this implementation.
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

impl std::fmt::Display for NodeCapabilityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(value) => write!(f, "Unknown(0x{:02x})", value),
            _ => write!(f, "{}", self.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NodeCapabilityType;

    #[test]
    fn node_capability_type_matches_neo_values() {
        assert_eq!(NodeCapabilityType::TcpServer.to_byte(), 0x01);
        assert_eq!(NodeCapabilityType::WsServer.to_byte(), 0x02);
        assert_eq!(NodeCapabilityType::DisableCompression.to_byte(), 0x03);
        assert_eq!(NodeCapabilityType::FullNode.to_byte(), 0x10);
        assert_eq!(NodeCapabilityType::ArchivalNode.to_byte(), 0x11);
        assert_eq!(NodeCapabilityType::Extension0.to_byte(), 0xF0);

        assert_eq!(
            NodeCapabilityType::from_byte(0x99),
            NodeCapabilityType::Unknown(0x99)
        );
    }

    #[test]
    fn node_capability_type_preserves_unknown_bytes() {
        let unknown = NodeCapabilityType::from_byte(0x99);
        assert_eq!(unknown.to_byte(), 0x99);
        assert_eq!(unknown.as_byte(), 0x99);
        assert_eq!(unknown.as_str(), "Unknown");
        assert!(!unknown.is_known());
        assert_eq!(unknown.to_string(), "Unknown(0x99)");
    }

    #[test]
    fn node_capability_type_serde_shape_matches_derived_enum() {
        let serialized = serde_json::to_string(&NodeCapabilityType::FullNode).unwrap();
        assert_eq!(serialized, "\"FullNode\"");

        let unknown = NodeCapabilityType::Unknown(0x99);
        let serialized_unknown = serde_json::to_string(&unknown).unwrap();
        assert_eq!(serialized_unknown, "{\"Unknown\":153}");

        let deserialized: NodeCapabilityType = serde_json::from_str(&serialized_unknown).unwrap();
        assert_eq!(deserialized, unknown);
    }
}

//! Capability type identifiers (mirrors `Neo.Network.P2P.Capabilities.NodeCapabilityType`).

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

impl std::fmt::Display for NodeCapabilityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(v) => write!(f, "Unknown(0x{:02x})", v),
            _ => write!(f, "{}", self.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_capability_type_values() {
        assert_eq!(NodeCapabilityType::TcpServer.to_byte(), 0x01);
        assert_eq!(NodeCapabilityType::WsServer.to_byte(), 0x02);
        assert_eq!(NodeCapabilityType::DisableCompression.to_byte(), 0x03);
        assert_eq!(NodeCapabilityType::FullNode.to_byte(), 0x10);
        assert_eq!(NodeCapabilityType::ArchivalNode.to_byte(), 0x11);
        assert_eq!(NodeCapabilityType::Extension0.to_byte(), 0xF0);
    }

    #[test]
    fn test_node_capability_type_from_byte() {
        assert_eq!(
            NodeCapabilityType::from_byte(0x01),
            NodeCapabilityType::TcpServer
        );
        assert_eq!(
            NodeCapabilityType::from_byte(0x10),
            NodeCapabilityType::FullNode
        );
        assert_eq!(
            NodeCapabilityType::from_byte(0x99),
            NodeCapabilityType::Unknown(0x99)
        );
    }

    #[test]
    fn test_node_capability_type_roundtrip() {
        for cap in [
            NodeCapabilityType::TcpServer,
            NodeCapabilityType::WsServer,
            NodeCapabilityType::DisableCompression,
            NodeCapabilityType::FullNode,
            NodeCapabilityType::ArchivalNode,
            NodeCapabilityType::Extension0,
        ] {
            let byte = cap.to_byte();
            let recovered = NodeCapabilityType::from_byte(byte);
            assert_eq!(cap, recovered);
        }
    }

    #[test]
    fn test_node_capability_type_is_known() {
        assert!(NodeCapabilityType::TcpServer.is_known());
        assert!(NodeCapabilityType::FullNode.is_known());
        assert!(!NodeCapabilityType::Unknown(0x99).is_known());
    }

    #[test]
    fn test_node_capability_type_display() {
        assert_eq!(NodeCapabilityType::TcpServer.to_string(), "TcpServer");
        assert_eq!(NodeCapabilityType::FullNode.to_string(), "FullNode");
        assert_eq!(
            NodeCapabilityType::Unknown(0x99).to_string(),
            "Unknown(0x99)"
        );
    }
}

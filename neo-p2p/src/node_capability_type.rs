//! Capability type identifiers (mirrors `Neo.Network.P2P.Capabilities.NodeCapabilityType`).

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

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TcpServer => "TcpServer",
            Self::WsServer => "WsServer",
            Self::DisableCompression => "DisableCompression",
            Self::FullNode => "FullNode",
            Self::ArchivalNode => "ArchivalNode",
            Self::Extension0 => "Extension0",
            Self::Unknown(_) => "Unknown",
        }
    }
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

//! Capability container implementation (mirrors `NodeCapability.cs`).

use crate::neo_io::{helper, BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

use super::node_capability_type::NodeCapabilityType;

/// Maximum number of bytes allowed for unknown capability payloads (matches C# `UnknownCapability.MaxDataSize`).
pub const MAX_UNKNOWN_CAPABILITY_DATA: usize = 1024;

/// Describes the advertised features of a Neo node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeCapability {
    /// The node is listening on a TCP port.
    TcpServer { port: u16 },
    /// The node is listening on a WebSocket port.
    WsServer { port: u16 },
    /// The node disables peer-to-peer compression.
    DisableCompression,
    /// The node maintains the full current state.
    FullNode { start_height: u32 },
    /// The node stores full historical blocks.
    ArchivalNode,
    /// Any capability not recognised by this implementation.
    Unknown {
        ty: NodeCapabilityType,
        data: Vec<u8>,
    },
}

impl NodeCapability {
    /// Creates a TCP server capability descriptor.
    pub fn tcp_server(port: u16) -> Self {
        Self::TcpServer { port }
    }

    /// Creates a WebSocket server capability descriptor.
    pub fn ws_server(port: u16) -> Self {
        Self::WsServer { port }
    }

    /// Creates a full node capability descriptor.
    pub fn full_node(start_height: u32) -> Self {
        Self::FullNode { start_height }
    }

    /// Creates an archival node capability descriptor.
    pub fn archival_node() -> Self {
        Self::ArchivalNode
    }

    /// Creates a disable-compression capability descriptor.
    pub fn disable_compression() -> Self {
        Self::DisableCompression
    }

    /// Creates an unknown capability from a raw identifier byte.
    pub fn unknown_from_byte(raw_type: u8, data: Vec<u8>) -> Self {
        Self::unknown(NodeCapabilityType::from_byte(raw_type), data)
    }

    /// Creates an unknown capability from a capability type.
    pub fn unknown(ty: NodeCapabilityType, data: Vec<u8>) -> Self {
        match ty {
            NodeCapabilityType::TcpServer
            | NodeCapabilityType::WsServer
            | NodeCapabilityType::DisableCompression
            | NodeCapabilityType::FullNode
            | NodeCapabilityType::ArchivalNode => panic!(
                "known capability {:?} should not be constructed via unknown()",
                ty
            ),
            NodeCapabilityType::Extension0 | NodeCapabilityType::Unknown(_) => {
                if data.len() > MAX_UNKNOWN_CAPABILITY_DATA {
                    panic!("unknown capability data too large: {} bytes", data.len());
                }
                Self::Unknown { ty, data }
            }
        }
    }

    /// Returns the capability type (including unknown identifiers).
    pub fn capability_type(&self) -> NodeCapabilityType {
        match self {
            Self::TcpServer { .. } => NodeCapabilityType::TcpServer,
            Self::WsServer { .. } => NodeCapabilityType::WsServer,
            Self::DisableCompression => NodeCapabilityType::DisableCompression,
            Self::FullNode { .. } => NodeCapabilityType::FullNode,
            Self::ArchivalNode => NodeCapabilityType::ArchivalNode,
            Self::Unknown { ty, .. } => *ty,
        }
    }

    pub fn deserialize_from(reader: &mut MemoryReader) -> IoResult<Self> {
        <Self as Serializable>::deserialize(reader)
    }

    fn ensure_zero_byte(reader: &mut MemoryReader, context: &str) -> IoResult<()> {
        let marker = reader.read_u8()?;
        if marker != 0 {
            return Err(IoError::format_exception(
                "NodeCapability::deserialize",
                &format!("{context} capability contains unexpected data"),
            ));
        }
        Ok(())
    }
}

impl Serializable for NodeCapability {
    fn size(&self) -> usize {
        match self {
            Self::TcpServer { .. } | Self::WsServer { .. } => 1 + 2,
            Self::DisableCompression | Self::ArchivalNode => 1 + 1,
            Self::FullNode { .. } => 1 + 4,
            Self::Unknown { data, .. } => 1 + helper::get_var_size(data.len() as u64) + data.len(),
        }
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        match self {
            Self::TcpServer { port } => {
                writer.write_u8(NodeCapabilityType::TcpServer.to_byte())?;
                writer.write_u16(*port)?;
            }
            Self::WsServer { port } => {
                writer.write_u8(NodeCapabilityType::WsServer.to_byte())?;
                writer.write_u16(*port)?;
            }
            Self::DisableCompression => {
                writer.write_u8(NodeCapabilityType::DisableCompression.to_byte())?;
                writer.write_u8(0)?;
            }
            Self::FullNode { start_height } => {
                writer.write_u8(NodeCapabilityType::FullNode.to_byte())?;
                writer.write_u32(*start_height)?;
            }
            Self::ArchivalNode => {
                writer.write_u8(NodeCapabilityType::ArchivalNode.to_byte())?;
                writer.write_u8(0)?;
            }
            Self::Unknown { ty, data } => {
                writer.write_u8(ty.to_byte())?;
                writer.write_var_bytes(data)?;
            }
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let type_byte = reader.read_u8()?;
        let capability_type = NodeCapabilityType::from_byte(type_byte);

        let capability = match capability_type {
            NodeCapabilityType::TcpServer => {
                let port = reader.read_u16()?;
                Self::tcp_server(port)
            }
            NodeCapabilityType::WsServer => {
                let port = reader.read_u16()?;
                Self::ws_server(port)
            }
            NodeCapabilityType::DisableCompression => {
                Self::ensure_zero_byte(reader, "DisableCompression")?;
                Self::disable_compression()
            }
            NodeCapabilityType::FullNode => {
                let start_height = reader.read_u32()?;
                Self::full_node(start_height)
            }
            NodeCapabilityType::ArchivalNode => {
                Self::ensure_zero_byte(reader, "ArchivalNode")?;
                Self::archival_node()
            }
            NodeCapabilityType::Extension0 | NodeCapabilityType::Unknown(_) => {
                let data = reader.read_var_bytes(MAX_UNKNOWN_CAPABILITY_DATA)?;
                Self::Unknown {
                    ty: capability_type,
                    data,
                }
            }
        };

        Ok(capability)
    }
}

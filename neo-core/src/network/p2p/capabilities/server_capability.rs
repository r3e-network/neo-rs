//! Server capability helpers (mirrors `ServerCapability.cs`).

use super::node_capability::NodeCapability;
use super::node_capability_type::NodeCapabilityType;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

/// Indicates that the node exposes a server endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCapability {
    kind: NodeCapabilityType,
    port: u16,
}

impl ServerCapability {
    pub fn new(kind: NodeCapabilityType, port: u16) -> IoResult<Self> {
        match kind {
            NodeCapabilityType::TcpServer | NodeCapabilityType::WsServer => Ok(Self { kind, port }),
            _ => Err(IoError::invalid_data(
                "ServerCapability can only be TcpServer or WsServer",
            )),
        }
    }

    pub fn tcp(port: u16) -> Self {
        Self {
            kind: NodeCapabilityType::TcpServer,
            port,
        }
    }

    pub fn ws(port: u16) -> Self {
        Self {
            kind: NodeCapabilityType::WsServer,
            port,
        }
    }

    pub fn capability_type(&self) -> NodeCapabilityType {
        self.kind
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl From<ServerCapability> for NodeCapability {
    fn from(value: ServerCapability) -> Self {
        match value.kind {
            NodeCapabilityType::TcpServer => NodeCapability::TcpServer { port: value.port },
            NodeCapabilityType::WsServer => NodeCapability::WsServer { port: value.port },
            other => NodeCapability::Unknown {
                ty: other,
                data: Vec::new(),
            },
        }
    }
}

impl TryFrom<&NodeCapability> for ServerCapability {
    type Error = &'static str;

    fn try_from(value: &NodeCapability) -> Result<Self, Self::Error> {
        match value {
            NodeCapability::TcpServer { port } => Ok(Self::tcp(*port)),
            NodeCapability::WsServer { port } => Ok(Self::ws(*port)),
            _ => Err("NodeCapability is not a server capability"),
        }
    }
}

impl Serializable for ServerCapability {
    fn size(&self) -> usize {
        NodeCapability::from(self.clone()).size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&NodeCapability::from(self.clone()), writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let capability = <NodeCapability as Serializable>::deserialize(reader)?;
        ServerCapability::try_from(&capability).map_err(IoError::invalid_data)
    }
}

pub fn tcp_server(port: u16) -> NodeCapability {
    NodeCapability::from(ServerCapability::tcp(port))
}

pub fn ws_server(port: u16) -> NodeCapability {
    NodeCapability::from(ServerCapability::ws(port))
}

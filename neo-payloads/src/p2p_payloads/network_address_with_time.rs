//! Network address descriptor with timestamp (mirrors `NetworkAddressWithTime.cs`).

use super::node_capability::{NodeCapabilities, NodeCapability};
use super::version_payload::MAX_CAPABILITIES;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Sent with an AddrPayload to respond to GetAddr messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAddressWithTime {
    /// The time when connected to the node.
    pub timestamp: u32,
    /// The address of the node.
    pub address: IpAddr,
    /// The capabilities of the node.
    pub capabilities: Vec<NodeCapability>,
}

impl NetworkAddressWithTime {
    pub fn new(timestamp: u32, address: IpAddr, capabilities: Vec<NodeCapability>) -> Self {
        Self {
            timestamp,
            address,
            capabilities,
        }
    }

    /// Gets the endpoint of the TCP server.
    pub fn endpoint(&self) -> Option<SocketAddr> {
        self.capabilities.iter().find_map(|cap| match cap {
            NodeCapability::TcpServer { port } => Some(SocketAddr::new(self.address, *port)),
            NodeCapability::WsServer { port } => Some(SocketAddr::new(self.address, *port)),
            _ => None,
        })
    }

    fn map_to_ipv6(addr: &IpAddr) -> [u8; 16] {
        match addr {
            IpAddr::V4(v4) => {
                let octets = v4.octets();
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, octets[0], octets[1], octets[2],
                    octets[3],
                ]
            }
            IpAddr::V6(v6) => v6.octets(),
        }
    }

    fn unmap_from_ipv6(bytes: &[u8; 16]) -> IpAddr {
        if bytes[0..10] == [0; 10] && bytes[10..12] == [0xff, 0xff] {
            IpAddr::V4(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]))
        } else {
            IpAddr::V6(Ipv6Addr::from(*bytes))
        }
    }
}

impl Serializable for NetworkAddressWithTime {
    fn size(&self) -> usize {
        4 + // timestamp
        16 + // mapped address
        NodeCapabilities::node_capabilities_size(&self.capabilities)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.timestamp)?;
        writer.write_bytes(&Self::map_to_ipv6(&self.address))?;

        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(IoError::invalid_data("Too many capabilities"));
        }

        NodeCapabilities::serialize_node_capabilities(&self.capabilities, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let timestamp = reader.read_u32()?;

        let addr_array = reader.read_array::<16>()?;
        let address = Self::unmap_from_ipv6(&addr_array);

        let capabilities = NodeCapabilities::deserialize_node_capabilities(reader, MAX_CAPABILITIES)?;

        Ok(Self {
            timestamp,
            address,
            capabilities,
        })
    }
}

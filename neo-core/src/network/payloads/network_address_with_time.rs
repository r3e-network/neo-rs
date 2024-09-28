use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::network::capabilities::{NodeCapability, NodeCapabilityType};

/// Sent with an `AddrPayload` to respond to `MessageCommand::GetAddr` messages.
pub struct NetworkAddressWithTime {
    /// The time when connected to the node.
    pub timestamp: u32,

    /// The address of the node.
    pub address: IpAddr,

    /// The capabilities of the node.
    pub capabilities: Vec<dyn NodeCapability>,
}

impl NetworkAddressWithTime {
    /// The `SocketAddr` of the Tcp server.
    pub fn end_point(&self) -> SocketAddr {
        let port = self.capabilities
            .iter()
            .find(|c| c.capability_type() == NodeCapabilityType::TcpServer)
            .and_then(|c| c.as_server_capability())
            .map(|sc| sc.port())
            .unwrap_or(0);
        SocketAddr::new(self.address, port)
    }

    /// Creates a new instance of the `NetworkAddressWithTime` struct.
    pub fn new(address: IpAddr, timestamp: u32, capabilities: Vec<dyn NodeCapability>) -> Self {
        Self {
            timestamp,
            address,
            capabilities,
        }
    }

    /// Creates a new instance with the current timestamp.
    pub fn create(address: IpAddr, capabilities: Vec<dyn NodeCapability>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as u32;
        Self::new(address, timestamp, capabilities)
    }
}

impl SerializableTrait for NetworkAddressWithTime {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.timestamp);
        let ipv6 = match self.address {
            IpAddr::V4(ipv4) => ipv4.to_ipv6_mapped(),
            IpAddr::V6(ipv6) => ipv6,
        };
        writer.write_fixed_bytes(&ipv6.octets());
        writer.write_var_bytes(&self.capabilities);
    }

    fn deserialize(reader: &mut BinaryReader) -> Result<Self, std::io::Error> {
        let timestamp = reader.read_u32()?;
        let ip_bytes = reader.read_fixed_bytes(16)?;
        let address = IpAddr::V6(Ipv6Addr::from(ip_bytes));
        let address = if address.is_ipv4() {
            address
        }else{
            IpAddr::V4(address)
        };
        let capabilities = reader.read_var_bytes()?;
        let capabilities: Vec<dyn NodeCapability> = capabilities
            .into_iter()
            .map(NodeCapability::deserialize)
            .collect::<Result<_, _>>()?;

        if capabilities.iter().map(|c| c.capability_type()).collect::<std::collections::HashSet<_>>().len() != capabilities.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Duplicate capability vm_types"));
        }

        Ok(Self {
            timestamp,
            address,
            capabilities,
        })
    }
}

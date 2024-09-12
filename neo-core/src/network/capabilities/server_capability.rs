
use super::NodeCapability;
use super::NodeCapabilityType;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;

/// Indicates that the node is a server.
pub struct ServerCapability {
    /// Indicates the port that the node is listening on.
    pub port: u16,
    capability_type: NodeCapabilityType,
}

impl ServerCapability {
    /// Creates a new instance of the ServerCapability struct.
    ///
    /// # Arguments
    ///
    /// * `capability_type` - The type of the ServerCapability. It must be NodeCapabilityType::TcpServer or NodeCapabilityType::WsServer
    /// * `port` - The port that the node is listening on.
    ///
    /// # Panics
    ///
    /// Panics if the capability_type is not TcpServer or WsServer.
    pub fn new(capability_type: NodeCapabilityType, port: u16) -> Self {
        match capability_type {
            NodeCapabilityType::TcpServer | NodeCapabilityType::WsServer => {},
            _ => panic!("Invalid capability type for ServerCapability"),
        }

        Self { port, capability_type }
    }
}

impl NodeCapability for ServerCapability {
    fn capability_type(&self) -> NodeCapabilityType {
        self.capability_type
    }

    fn size(&self) -> usize {
        std::mem::size_of::<NodeCapabilityType>() + // Type
        std::mem::size_of::<u16>()                  // Port
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) {
        self.port = reader.read_u16().unwrap();
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) {
        writer.write_u16(self.port);
    }
}

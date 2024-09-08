

use std::io;
use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use crate::network::Capabilities::{FullNodeCapability, NodeCapabilityType, ServerCapability};

/// Represents the capabilities of a NEO node.
pub trait NodeCapability {
    /// Indicates the type of the NodeCapability.
    fn capability_type(&self) -> NodeCapabilityType;

    /// Returns the size of the serialized NodeCapability.
    fn size(&self) -> usize {
        std::mem::size_of::<NodeCapabilityType>() // Type
    }

    /// Deserializes the NodeCapability object from a MemoryReader.
    fn deserialize_without_type(&mut self, reader: &mut MemoryReader);

    /// Serializes the NodeCapability object to a BinaryWriter.
    fn serialize_without_type(&self, writer: &mut BinaryWriter);

    /// Deserializes an NodeCapability object from a MemoryReader.
    fn deserialize_from(reader: &mut MemoryReader) -> io::Result<Box<dyn NodeCapability>> {
        let capability_type = NodeCapabilityType::from(reader.read_u8()?);
        let mut capability: Box<dyn NodeCapability> = match capability_type {
            NodeCapabilityType::TcpServer | NodeCapabilityType::WsServer => {
                Box::new(ServerCapability::new(capability_type))
            }
            NodeCapabilityType::FullNode => Box::new(FullNodeCapability::new(0)),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid capability type")),
        };
        capability.deserialize_without_type(reader);
        Ok(capability)
    }

    /// Serializes the NodeCapability object to a BinaryWriter.
    fn serialize(&self, writer: &mut BinaryWriter) -> io::Result<()> {
        writer.write_u8(self.capability_type() as u8);
        self.serialize_without_type(writer);
        Ok(())
    }
}

// Note: ServerCapability and FullNodeCapability implementations are not shown here,
// but they should be implemented separately.

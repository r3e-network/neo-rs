use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;
use super::NodeCapability;
use super::NodeCapabilityType;

/// Indicates that a node has complete block data.
pub struct FullNodeCapability {
    /// Indicates the current block height of the node.
    pub start_height: u32,
}

impl FullNodeCapability {
    /// Creates a new instance of the FullNodeCapability struct.
    ///
    /// # Arguments
    ///
    /// * `start_height` - The current block height of the node.
    pub fn new(start_height: u32) -> Self {
        Self { start_height }
    }
}

impl NodeCapability for FullNodeCapability {
    fn capability_type(&self) -> NodeCapabilityType {
        NodeCapabilityType::FullNode
    }

    fn size(&self) -> usize {
        std::mem::size_of::<NodeCapabilityType>() + // Type
        std::mem::size_of::<u32>()                  // Start Height
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) {
        self.start_height = reader.read_u32().unwrap();
    }

    fn serialize_without_type(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.start_height);
    }
}

use crate::io::binary_writer::BinaryWriter;
use crate::io::memory_reader::MemoryReader;

/// Represents NEO objects that can be serialized.
pub trait SerializableTrait {
    /// The size of the object in bytes after serialization.
    fn size(&self) -> usize;

    /// Serializes the object using the specified `BinaryWriter`.
    ///
    /// # Arguments
    ///
    /// * `writer` - The `BinaryWriter` for writing data.
    fn serialize(&self, writer: &mut BinaryWriter);

    /// Deserializes the object using the specified `MemoryReader`.
    ///
    /// # Arguments
    ///
    /// * `reader` - The `MemoryReader` for reading data.
    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error>;
}

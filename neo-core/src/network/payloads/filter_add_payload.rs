use std::borrow::Cow;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;

/// This message is sent to update the items for the BloomFilter.
pub struct FilterAddPayload {
    /// The items to be added.
    pub data: Cow<'static, [u8]>,
}


impl SerializableTrait for FilterAddPayload {
    fn size(&self) -> usize {
        self.data.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_var_bytes(&self.data)
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let data = reader.read_var_bytes(520)?;
        Ok(FilterAddPayload { data: Cow::Owned(data) })
    }
}

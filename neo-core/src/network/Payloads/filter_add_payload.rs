use std::borrow::Cow;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;

/// This message is sent to update the items for the BloomFilter.
pub struct FilterAddPayload {
    /// The items to be added.
    pub data: Cow<'static, [u8]>,
}

impl FilterAddPayload {
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

impl ISerializable for FilterAddPayload {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), std::io::Error> {
        writer.write_var_bytes(&self.data)?;
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let data = reader.read_var_bytes(520)?;
        Ok(FilterAddPayload { data: Cow::Owned(data) })
    }
}

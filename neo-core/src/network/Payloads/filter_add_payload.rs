use neo_core::io::{Serializable, MemoryReader, BinaryWriter};
use std::borrow::Cow;

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
    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let data = reader.read_var_bytes(520)?;
        Ok(FilterAddPayload { data: Cow::Owned(data) })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), std::io::Error> {
        writer.write_var_bytes(&self.data)?;
        Ok(())
    }
}

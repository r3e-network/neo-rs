use std::io;
use std::mem::size_of;
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;

/// This message is sent to load the BloomFilter.
pub struct FilterLoadPayload {
    /// The data of the BloomFilter.
    pub filter: Vec<u8>,

    /// The number of hash functions used by the BloomFilter.
    pub k: u8,

    /// Used to generate the seeds of the murmur hash functions.
    pub tweak: u32,
}

impl FilterLoadPayload {
    /// Creates a new instance of the FilterLoadPayload struct.
    ///
    /// # Arguments
    ///
    /// * `filter` - The fields in the filter will be copied to the payload.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn create(filter: &BloomFilter) -> Self {
        let mut buffer = vec![0u8; filter.m / 8];
        filter.get_bits(&mut buffer);
        FilterLoadPayload {
            filter: buffer,
            k: filter.k as u8,
            tweak: filter.tweak,
        }
    }

}

impl SerializableTrait for FilterLoadPayload {
    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let filter = reader.read_var_bytes(36000)?;
        let k = reader.read_u8()?;
        if k > 50 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "K is too large"));
        }
        let tweak = reader.read_u32()?;
        Ok(FilterLoadPayload { filter, k, tweak })
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_var_bytes(&self.filter)?;
        writer.write_u8(self.k)?;
        writer.write_u32(self.tweak)?;
        Ok(())
    }
    
    fn size(&self) -> usize {
        self.filter.len() + size_of::<u8>() + size_of::<u32>()
    }
}

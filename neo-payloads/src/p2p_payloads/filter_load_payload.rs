use neo_crypto::bloom_filter::BloomFilter;
use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Maximum filter size (36000 bytes)
const MAX_FILTER_SIZE: usize = 36000;

/// Maximum number of hash functions (50)
const MAX_K: u8 = 50;

/// This message is sent to load the BloomFilter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterLoadPayload {
    /// The data of the BloomFilter.
    pub filter: Vec<u8>,

    /// The number of hash functions used by the BloomFilter.
    pub k: u8,

    /// Used to generate the seeds of the murmur hash functions.
    pub tweak: u32,
}

impl FilterLoadPayload {
    /// Creates a new filter load payload.
    pub fn new(filter: Vec<u8>, k: u8, tweak: u32) -> Self {
        Self { filter, k, tweak }
    }

    /// Creates a payload from an existing bloom filter instance.
    pub fn create_from_bloom_filter(filter: &BloomFilter) -> Self {
        let mut filter_bits = filter.bits();
        if filter_bits.len() > MAX_FILTER_SIZE {
            filter_bits.truncate(MAX_FILTER_SIZE);
        }

        Self {
            filter: filter_bits,
            k: filter.hash_functions().min(MAX_K as usize) as u8,
            tweak: filter.tweak(),
        }
    }
}

impl Serializable for FilterLoadPayload {
    fn size(&self) -> usize {
        get_var_size_bytes(&self.filter) +
        1 + // K
        4 // Tweak
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        // Write filter as var bytes
        if self.filter.len() > MAX_FILTER_SIZE {
            return Err(IoError::invalid_data("Filter too large"));
        }
        writer.write_var_bytes(&self.filter)?;

        writer.write_u8(self.k)?;
        writer.write_u32(self.tweak)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let filter = reader.read_var_bytes(MAX_FILTER_SIZE)?;

        let k = reader.read_u8()?;
        if k > MAX_K {
            return Err(IoError::invalid_data("K value exceeds maximum"));
        }

        let tweak = reader.read_u32()?;

        Ok(Self { filter, k, tweak })
    }
}

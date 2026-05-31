use super::{Header, Witness};
use crate::error::CoreResult;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::{UInt160, UInt256};

impl Header {
    /// Returns the unsigned serialization used for hashing.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(err) => {
                tracing::error!("Failed to serialize header unsigned data: {err}");
                Vec::new()
            }
        }
    }

    /// Returns the unsigned serialization used for hashing, or an error if the
    /// header cannot be represented on the wire.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Serialize without witness.
    pub(super) fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        writer.write_serializable(&self.prev_hash)?;
        writer.write_serializable(&self.merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        writer.write_serializable(&self.next_consensus)?;
        Ok(())
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        4 + 32
            + 32
            + 8
            + 8
            + 4
            + 1
            + 20
            + crate::neo_io::serializable::helper::get_var_size(1)
            + self.witness.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1 for header)
        writer.write_var_uint(1)?;
        writer.write_serializable(&self.witness)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u32()?;
        if version > 0 {
            return Err(IoError::invalid_data("unsupported header version"));
        }
        let prev_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let merkle_root = <UInt256 as Serializable>::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_u8()?;
        let next_consensus = <UInt160 as Serializable>::deserialize(reader)?;

        // Read witness count (should be 1)
        let witness_count = reader.read_var_uint()?;
        if witness_count != 1 {
            return Err(IoError::invalid_data("Invalid witness count for header"));
        }

        let witness = <Witness as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witness,
            _hash: parking_lot::Mutex::new(None),
        })
    }
}

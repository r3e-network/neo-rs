// Unsigned fields come from `crate::ledger::header_wire`, the same layout
// `BlockHeader` hashes. This file only frames the single header witness.

use super::{Header, Witness};
use crate::error::CoreResult;
use crate::ledger::header_wire::UnsignedHeader;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

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
        self.unsigned().write(writer)
    }

    fn unsigned(&self) -> UnsignedHeader {
        UnsignedHeader {
            version: self.version,
            previous_hash: self.prev_hash,
            merkle_root: self.merkle_root,
            timestamp: self.timestamp,
            nonce: self.nonce,
            index: self.index,
            primary_index: self.primary_index,
            next_consensus: self.next_consensus,
        }
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        UnsignedHeader::WIRE_SIZE
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
        let fields = UnsignedHeader::read(reader)?;
        if fields.version > 0 {
            return Err(IoError::invalid_data("unsupported header version"));
        }
        let version = fields.version;
        let prev_hash = fields.previous_hash;
        let merkle_root = fields.merkle_root;
        let timestamp = fields.timestamp;
        let nonce = fields.nonce;
        let index = fields.index;
        let primary_index = fields.primary_index;
        let next_consensus = fields.next_consensus;

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
            _hash: None,
        })
    }
}

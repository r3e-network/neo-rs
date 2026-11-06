// The common unsigned layout is owned by crate::header_wire.
use super::{Header, Witness};
use crate::header_wire::UnsignedHeader;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

impl Header {
    /// Returns unsigned bytes, or the historical empty fallback on failure.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(err) => {
                tracing::error!("Failed to serialize header unsigned data: {err}");
                Vec::new()
            }
        }
    }
    /// Returns the unsigned hash preimage with low-level wire errors.
    pub fn try_get_hash_data(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        Ok(writer.into_bytes())
    }
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
            + neo_io::serializable::helper::get_var_size(1)
            + self.witness.size()
    }
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        writer.write_var_uint(1)?;
        writer.write_serializable(&self.witness)?;
        Ok(())
    }
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let fields = UnsignedHeader::read(reader)?;
        if fields.version > 0 {
            return Err(IoError::invalid_data("unsupported header version"));
        }
        let witness_count = reader.read_var_uint()?;
        if witness_count != 1 {
            return Err(IoError::invalid_data("Invalid witness count for header"));
        }
        let witness = <Witness as Serializable>::deserialize(reader)?;
        Ok(Self {
            version: fields.version,
            prev_hash: fields.previous_hash,
            merkle_root: fields.merkle_root,
            timestamp: fields.timestamp,
            nonce: fields.nonce,
            index: fields.index,
            primary_index: fields.primary_index,
            next_consensus: fields.next_consensus,
            witness,
            _hash: None,
        })
    }
}

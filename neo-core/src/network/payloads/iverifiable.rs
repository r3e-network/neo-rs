use std::io::Write;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::network::calculate_hash;
use crate::network::payloads::Witness;
use crate::persistence::DataCache;
use neo_type::H160;
use neo_type::H256;

/// Represents an object that can be verified in the NEO network.
pub trait IVerifiable: SerializableTrait {
    type Error;

    /// The hash of the IVerifiable object.
    fn hash(&self) -> H256 {
        calculate_hash(&self)
    }

    /// The witnesses of the IVerifiable object.
    fn witnesses(&self) -> &[Witness];
    fn set_witnesses(&mut self, witnesses: Vec<Witness>);

    /// Deserializes the part of the IVerifiable object other than Witnesses.
    fn deserialize_unsigned(reader: &mut MemoryReader) -> Result<Self, Self::Error>;

    /// Gets the script hashes that should be verified for this IVerifiable object.
    fn get_script_hashes_for_verifying(&self, snapshot: &dyn DataCache) -> Vec<H160>;

    /// Serializes the part of the IVerifiable object other than Witnesses.
    fn serialize_unsigned(&self, writer: &mut BinaryWriter);
}

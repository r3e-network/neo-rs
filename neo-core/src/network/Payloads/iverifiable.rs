use std::io::Write;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::Witness;
use crate::persistence::DataCache;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents an object that can be verified in the NEO network.
pub trait IVerifiable: ISerializable {
    /// The hash of the IVerifiable object.
    fn hash(&self) -> UInt256 {
        self.calculate_hash()
    }

    /// The witnesses of the IVerifiable object.
    fn witnesses(&self) -> &[Witness];
    fn set_witnesses(&mut self, witnesses: Vec<Witness>);

    /// Deserializes the part of the IVerifiable object other than Witnesses.
    fn deserialize_unsigned(&mut self, reader: &mut MemoryReader);

    /// Gets the script hashes that should be verified for this IVerifiable object.
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160>;

    /// Serializes the part of the IVerifiable object other than Witnesses.
    fn serialize_unsigned<W: Write>(&self, writer: &mut W);
}

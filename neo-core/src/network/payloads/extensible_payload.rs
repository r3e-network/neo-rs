use std::collections::HashSet;
use std::hash::Hasher;
use std::io::{Read, Write};
use crate::io::iserializable::ISerializable;
use crate::network::payloads::{IInventory, IVerifiable, InventoryType, Witness};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents an extensible message that can be relayed.
pub struct ExtensiblePayload {
    /// The category of the extension.
    pub category: String,

    /// Indicates that the payload is only valid when the block height is greater than or equal to this value.
    pub valid_block_start: u32,

    /// Indicates that the payload is only valid when the block height is less than this value.
    pub valid_block_end: u32,

    /// The sender of the payload.
    pub sender: UInt160,

    /// The data of the payload.
    pub data: Vec<u8>,

    /// The witness of the payload. It must match the `sender`.
    pub witness: Witness,

    hash: Option<UInt256>,
}

impl IInventory for ExtensiblePayload {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Extensible
    }

    fn hash(&self) -> UInt256 {
        if let Some(hash) = self.hash {
            hash
        } else {
            let hash = self.calculate_hash();
            self.hash = Some(hash);
            hash
        }
    }
}

impl ISerializable for ExtensiblePayload {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        self.serialize_unsigned(writer)?;
        writer.write_u8(1);
        self.witness.serialize(writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let mut payload = ExtensiblePayload::deserialize_unsigned(reader)?;
        if reader.read_u8()? != 1 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid format"));
        }
        payload.witness = Witness::deserialize(reader)?;
        Ok(payload)
    }
}

impl IVerifiable for ExtensiblePayload {
    fn deserialize_unsigned(&mut self, reader: &mut impl Read) -> std::io::Result<()> {
        self.category = read_var_string(reader, 32)?;
        self.valid_block_start = reader.read_u32::<LittleEndian>()?;
        self.valid_block_end = reader.read_u32::<LittleEndian>()?;
        if self.valid_block_start >= self.valid_block_end {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid block range"));
        }
        self.sender = UInt160::deserialize(reader)?;
        self.data = read_var_bytes(reader)?;
        Ok(())
    }

    fn serialize_unsigned(&self, writer: &mut impl Write) -> std::io::Result<()> {
        write_var_string(writer, &self.category)?;
        writer.write_u32::<LittleEndian>(self.valid_block_start)?;
        writer.write_u32::<LittleEndian>(self.valid_block_end)?;
        self.sender.serialize(writer)?;
        write_var_bytes(writer, &self.data)?;
        Ok(())
    }

    fn get_script_hashes_for_verifying(&self, _snapshot: &dyn DataCache) -> Vec<UInt160> {
        vec![self.sender]
    }

    fn witnesses(&self) -> &[Witness] {
        todo!()
    }

    fn set_witnesses(&mut self, witnesses: Vec<Witness>) {
        todo!()
    }
}

impl ExtensiblePayload {
    pub fn size(&self) -> usize {
        var_size(&self.category) +
        std::mem::size_of::<u32>() +
        std::mem::size_of::<u32>() +
        UInt160::len() +
        var_size(&self.data) +
        1 + self.witness.size()
    }

    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &dyn DataCache, extensible_witness_white_list: &HashSet<UInt160>) -> bool {
        let height = NativeContract::Ledger.current_index(snapshot);
        if height < self.valid_block_start || height >= self.valid_block_end {
            return false;
        }
        if !extensible_witness_white_list.contains(&self.sender) {
            return false;
        }
        self.verify_witnesses(settings, snapshot, 0_06000000)
    }
}

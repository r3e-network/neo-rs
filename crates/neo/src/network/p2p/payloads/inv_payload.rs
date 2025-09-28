//! Inventory payload (mirrors `Neo.Network.P2P.Payloads.InvPayload`).

use super::inventory_type::InventoryType;
use crate::neo_config::HASH_SIZE;
use crate::neo_io::{helper, BinaryWriter, MemoryReader, Serializable};
use crate::uint256::UInt256;
use serde::{Deserialize, Serialize};

/// Maximum number of hashes allowed in a single payload.
pub const MAX_HASHES_COUNT: usize = 500;

/// Inventory relay payload containing hashes of announced objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvPayload {
    /// Type of inventory being advertised.
    pub inventory_type: InventoryType,
    /// Hashes carried by this message.
    pub hashes: Vec<UInt256>,
}

impl InvPayload {
    /// Creates a new payload from the provided hashes.
    pub fn new<T>(inventory_type: InventoryType, hashes: T) -> Self
    where
        T: Into<Vec<UInt256>>,
    {
        let hashes_vec = hashes.into();
        debug_assert!(hashes_vec.len() <= MAX_HASHES_COUNT);
        Self {
            inventory_type,
            hashes: hashes_vec,
        }
    }

    /// Convenience constructor (matches the C# `Create` helper).
    pub fn create(inventory_type: InventoryType, hashes: &[UInt256]) -> Self {
        Self::new(inventory_type, hashes.to_vec())
    }

    /// Splits an arbitrary collection of hashes into protocol-compliant payloads.
    pub fn create_group<I>(inventory_type: InventoryType, hashes: I) -> Vec<Self>
    where
        I: IntoIterator<Item = UInt256>,
    {
        let hashes_vec: Vec<UInt256> = hashes.into_iter().collect();
        if hashes_vec.is_empty() {
            return Vec::new();
        }

        hashes_vec
            .chunks(MAX_HASHES_COUNT)
            .map(|chunk| Self::new(inventory_type, chunk.to_vec()))
            .collect()
    }

    /// Returns `true` when no hashes are carried by the payload.
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Returns the number of hashes in the payload.
    pub fn len(&self) -> usize {
        self.hashes.len()
    }
}

impl Serializable for InvPayload {
    fn size(&self) -> usize {
        1 + helper::get_var_size(self.hashes.len() as u64) + self.hashes.len() * HASH_SIZE
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::Result<()> {
        writer.write_u8(self.inventory_type as u8)?;
        helper::serialize_array(&self.hashes, writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> neo_io::Result<Self> {
        let ty = match reader.read_u8()? {
            0x2b => InventoryType::Transaction,
            0x2c => InventoryType::Block,
            0xe0 => InventoryType::Consensus,
            other => {
                return Err(neo_io::Error::InvalidData(format!(
                    "Unsupported inventory type: {:#x}",
                    other
                )));
            }
        };

        let hashes = helper::deserialize_array::<UInt256>(reader, MAX_HASHES_COUNT)?;
        Ok(Self {
            inventory_type: ty,
            hashes,
        })
    }
}

impl Default for InvPayload {
    fn default() -> Self {
        Self {
            inventory_type: InventoryType::Transaction,
            hashes: Vec::new(),
        }
    }
}

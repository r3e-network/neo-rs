//! Inventory item types.
//!
//! This module provides inventory functionality exactly matching C# Neo InventoryItem.

use crate::{Error, Result};
use neo_core::UInt256;
use neo_io::{BinaryWriter, MemoryReader};
use serde::{Deserialize, Serialize};

/// Inventory item types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum InventoryType {
    Transaction = 0x2b,
    Block = 0x2c,
    Consensus = 0xe0,
}

/// Inventory item
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InventoryItem {
    /// Item type
    pub item_type: InventoryType,
    /// Item hash
    pub hash: UInt256,
}

impl InventoryItem {
    /// Creates a new inventory item
    pub fn new(item_type: InventoryType, hash: UInt256) -> Self {
        Self { item_type, hash }
    }
    
    /// Creates a transaction inventory item
    pub fn transaction(hash: UInt256) -> Self {
        Self::new(InventoryType::Transaction, hash)
    }
    
    /// Creates a block inventory item
    pub fn block(hash: UInt256) -> Self {
        Self::new(InventoryType::Block, hash)
    }
    
    /// Creates a consensus inventory item
    pub fn consensus(hash: UInt256) -> Self {
        Self::new(InventoryType::Consensus, hash)
    }
}

/// Implementation of neo_io::Serializable trait for InventoryItem
impl neo_io::Serializable for InventoryItem {
    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::Result<()> {
        writer.write_u8(self.item_type as u8)?;
        writer.write_bytes(self.hash.as_bytes())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> neo_io::Result<Self> {
        let item_type = match reader.read_byte()? {
            0x2b => InventoryType::Transaction,
            0x2c => InventoryType::Block,
            0xe0 => InventoryType::Consensus,
            _ => return Err(neo_io::Error::InvalidData("Invalid inventory type".to_string())),
        };
        let hash_bytes = reader.read_bytes(32)?;
        let hash = UInt256::from_bytes(&hash_bytes)
            .map_err(|e| neo_io::Error::InvalidData(format!("Invalid hash: {}", e)))?;
        Ok(Self { item_type, hash })
    }

    fn size(&self) -> usize {
        1 + 32 // 1 byte for type + 32 bytes for hash
    }
}

impl InventoryItem {
    /// Serializes to binary format (Neo N3 compatible) - deprecated, use Serializable trait instead
    #[deprecated(note = "Use neo_io::Serializable trait instead")]
    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<()> {
        <Self as neo_io::Serializable>::serialize(self, writer)
            .map_err(|e| Error::Protocol(format!("Serialization error: {}", e)))
    }

    /// Deserializes from binary format (Neo N3 compatible) - deprecated, use Serializable trait instead  
    #[deprecated(note = "Use neo_io::Serializable trait instead")]
    pub fn deserialize(reader: &mut MemoryReader) -> Result<Self> {
        <Self as neo_io::Serializable>::deserialize(reader)
            .map_err(|e| Error::Protocol(format!("Deserialization error: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_item() {
        let hash = UInt256::zero();
        
        let tx_item = InventoryItem::transaction(hash);
        assert_eq!(tx_item.item_type, InventoryType::Transaction);
        assert_eq!(tx_item.hash, hash);
        
        let block_item = InventoryItem::block(hash);
        assert_eq!(block_item.item_type, InventoryType::Block);
        assert_eq!(block_item.hash, hash);
    }
} 
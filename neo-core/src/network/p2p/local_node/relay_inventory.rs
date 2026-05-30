//! Inventory items relayed across the P2P network.

use super::*;

/// Inventory items that can be relayed across the P2P network.
#[derive(Debug, Clone)]
pub enum RelayInventory {
    /// A complete block to relay.
    Block(Block),
    /// A transaction to relay.
    Transaction(Transaction),
    /// An extensible payload (consensus, oracle, etc.).
    Extensible(ExtensiblePayload),
}

impl RelayInventory {
    /// Returns the inventory type for this relay item.
    pub fn inventory_type(&self) -> InventoryType {
        match self {
            RelayInventory::Block(_) => InventoryType::Block,
            RelayInventory::Transaction(_) => InventoryType::Transaction,
            RelayInventory::Extensible(_) => InventoryType::Extensible,
        }
    }

    /// Serializes the inventory item to bytes for network transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        let result = match self {
            RelayInventory::Block(block) => Serializable::serialize(block, &mut writer),
            RelayInventory::Transaction(tx) => Serializable::serialize(tx, &mut writer),
            RelayInventory::Extensible(payload) => Serializable::serialize(payload, &mut writer),
        };
        if let Err(e) = result {
            tracing::error!("Failed to serialize inventory: {:?}", e);
            return Vec::new();
        }
        writer.into_bytes()
    }
}

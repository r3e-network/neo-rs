//! Inventory trait for network-relayable items.
//!
//! This trait represents items that can be relayed on the Neo P2P network
//! (blocks, transactions, extensible payloads). It extends `SerializablePayload`
//! with inventory type identification.

use crate::{InventoryType, SerializablePayload, UInt256};

/// Represents a message that can be relayed on the NEO network.
///
/// This trait combines serialization capabilities (`SerializablePayload`)
/// with inventory type identification for P2P message routing.
///
/// # Implementors
///
/// - `Block` (neo-core)
/// - `Transaction` (neo-core)
/// - `ExtensiblePayload` (neo-core)
pub trait Inventory: SerializablePayload {
    /// The type of the inventory (Block, Transaction, etc.).
    fn inventory_type(&self) -> InventoryType;

    /// Gets the hash of the inventory item.
    fn inventory_hash(&self) -> UInt256 {
        self.hash()
    }
}

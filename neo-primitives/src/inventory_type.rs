//! Inventory type identifiers (mirrors `Neo.Network.P2P.Payloads.InventoryType`).

use crate::protocol_enum_repr;
use serde::{Deserialize, Serialize};

protocol_enum_repr! {
    /// Represents the type of an inventory.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub InventoryType {
        /// Indicates that the inventory is a Transaction.
        Transaction = 0x2b => "TX",
        /// Indicates that the inventory is a Block.
        Block = 0x2c,
        /// Indicates that the inventory is an `ExtensiblePayload`.
        Extensible = 0x2e,
    }
}

#[cfg(test)]
#[path = "tests/inventory_type.rs"]
mod tests;

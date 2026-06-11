//! Inventory reverification request and its per-item record.
//!
//! `Reverify` and `ReverifyItem` are kept together: `ReverifyItem`
//! exists only as the element type of `Reverify::inventories`.

use serde::{Deserialize, Serialize};

use crate::inventory_payload::InventoryPayload;

/// Item to be reverified in the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverifyItem {
    /// The inventory payload to reverify.
    pub payload: InventoryPayload,
    /// Optional block index context.
    #[serde(default)]
    pub block_index: Option<u32>,
}

/// Request to reverify inventory items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reverify {
    /// Items to reverify.
    pub inventories: Vec<ReverifyItem>,
}

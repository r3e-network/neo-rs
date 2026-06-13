//! Inventory payload types for relay and verification.

use neo_payloads::{Block, InventoryType, Transaction, extensible_payload::ExtensiblePayload};
use serde::{Deserialize, Serialize};

/// Inventory payload types for relay and verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InventoryPayload {
    /// A block payload.
    Block(Box<Block>),
    /// A transaction payload.
    Transaction(Box<Transaction>),
    /// An extensible payload.
    Extensible(Box<ExtensiblePayload>),
    /// Raw inventory data with type.
    Raw(InventoryType, Vec<u8>),
}

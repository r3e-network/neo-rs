//! Result of a relay operation.

use super::*;

/// Result of a relay operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    /// Hash of the relayed inventory.
    pub hash: UInt256,
    /// Type of inventory that was relayed.
    pub inventory_type: InventoryType,
    /// Optional block index context.
    pub block_index: Option<u32>,
    /// Verification result.
    pub result: VerifyResult,
}

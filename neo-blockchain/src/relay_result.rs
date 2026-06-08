//! Result of a relay operation.

use neo_payloads::InventoryType;
use neo_primitives::verify_result::VerifyResult;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

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

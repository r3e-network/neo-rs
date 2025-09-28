//! Block Log State
//!
//! State management for block logs.

use serde::{Deserialize, Serialize};

/// Block Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockLogState {
    /// Block hash
    pub block_hash: String,
    /// Block index
    pub block_index: u32,
    /// Timestamp
    pub timestamp: u64,
}

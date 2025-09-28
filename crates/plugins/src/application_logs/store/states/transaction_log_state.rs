//! Transaction Log State
//!
//! State management for transaction logs.

use serde::{Deserialize, Serialize};

/// Transaction Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLogState {
    /// Transaction hash
    pub transaction_hash: String,
    /// Transaction type
    pub transaction_type: String,
    /// Timestamp
    pub timestamp: u64,
}

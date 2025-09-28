//! Transaction Engine Log State
//!
//! State management for transaction engine logs.

use serde::{Deserialize, Serialize};

/// Transaction Engine Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEngineLogState {
    /// Transaction hash
    pub transaction_hash: String,
    /// Engine type
    pub engine_type: String,
    /// Execution time
    pub execution_time: u64,
}

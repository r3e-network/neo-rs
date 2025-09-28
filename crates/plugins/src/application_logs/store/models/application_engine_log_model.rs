//! Application Engine Log Model
//!
//! Model for application engine logs.

use serde::{Deserialize, Serialize};

/// Application Engine Log Model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationEngineLogModel {
    /// Transaction hash
    pub transaction_hash: String,
    /// Script hash
    pub script_hash: String,
    /// Gas consumed
    pub gas_consumed: u64,
    /// Execution time
    pub execution_time: u64,
}

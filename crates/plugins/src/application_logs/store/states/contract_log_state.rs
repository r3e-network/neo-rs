//! Contract Log State
//!
//! State management for contract logs.

use serde::{Deserialize, Serialize};

/// Contract Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractLogState {
    /// Contract hash
    pub contract_hash: String,
    /// Contract name
    pub contract_name: String,
    /// Deployment timestamp
    pub deployment_timestamp: u64,
}

//! Engine Log State
//!
//! State management for engine logs.

use serde::{Deserialize, Serialize};

/// Engine Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineLogState {
    /// Engine type
    pub engine_type: String,
    /// Execution time
    pub execution_time: u64,
    /// Gas consumed
    pub gas_consumed: u64,
}

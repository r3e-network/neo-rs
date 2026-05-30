//! Consensus participation configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusSettings {
    /// Enable consensus participation
    #[serde(default)]
    pub enabled: bool,

    /// Path to wallet file for consensus
    pub wallet_path: Option<PathBuf>,

    /// Wallet password (should be loaded from a secure source, e.g. env/secret manager)
    #[serde(skip_serializing)]
    pub wallet_password: Option<String>,

    /// Consensus timeout multiplier
    #[serde(default = "default_timeout_multiplier")]
    pub timeout_multiplier: f64,
}

const fn default_timeout_multiplier() -> f64 {
    1.0
}

impl Default for ConsensusSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            wallet_path: None,
            wallet_password: None,
            timeout_multiplier: default_timeout_multiplier(),
        }
    }
}

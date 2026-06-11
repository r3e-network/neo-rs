//! Request to import blocks into the blockchain.

use neo_payloads::Block;
use serde::{Deserialize, Serialize};

/// Request to import blocks into the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Blocks to import.
    pub blocks: Vec<Block>,
    /// Whether to verify blocks before importing.
    pub verify: bool,
}

impl Default for Import {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            verify: true,
        }
    }
}

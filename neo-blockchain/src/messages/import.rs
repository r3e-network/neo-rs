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
    /// Whether this import is a trusted bulk-sync/bootstrap path.
    ///
    /// Bulk sync still executes the native persistence state transition and
    /// writes consensus-visible ledger records, but skips replay-only artifacts
    /// that local plugin/indexer hooks intentionally do not consume during cold
    /// bootstrap.
    #[serde(default)]
    pub bulk_sync: bool,
}

impl Default for Import {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            verify: true,
            bulk_sync: false,
        }
    }
}

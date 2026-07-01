//! Notification that a block has been persisted to storage.

use std::sync::Arc;

use neo_payloads::Block;

/// Notification that a block has been persisted to storage.
#[derive(Debug, Clone)]
pub struct PersistCompleted {
    /// The block that was persisted.
    pub block: Arc<Block>,
}

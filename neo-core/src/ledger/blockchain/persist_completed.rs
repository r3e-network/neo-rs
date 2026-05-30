//! Notification that a block has been persisted to storage.

use super::*;
use std::sync::Arc;

/// Notification that a block has been persisted to storage.
#[derive(Debug, Clone)]
pub struct PersistCompleted {
    /// The block that was persisted.
    pub block: Arc<Block>,
}

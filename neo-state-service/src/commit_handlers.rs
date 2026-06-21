//! Block-commit handler pipeline for the state service.
//!
//! Wires local MPT state-root persistence into the block persistence
//! pipeline:
//!
//! - On `Committing(block, snapshot, ...)` - projects the snapshot's
//!   tracked storage changes into the persisted MPT via
//!   [`StateStore::apply_snapshot_changes`].
//! - On explicit revert handling - drops any candidate state roots whose
//!   block index falls in the reverting range via [`StateStore::discard`].
//!
//! The handler is intentionally a thin adapter over [`StateStore`], so the
//! C# `Blockchain_Committing_Handler` filtering rules live in one place.

use crate::state_store::StateStore;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_storage::DataCache;
use std::any::Any;
use std::sync::Arc;
use tracing::{debug, warn};

/// Handlers for wiring state-root MPT persistence into block persistence.
pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
}

impl StateServiceCommitHandlers {
    /// Constructs a new pipeline backed by the supplied state store.
    pub fn new(state_store: Arc<StateStore>) -> Self {
        Self { state_store }
    }

    /// Returns a clone of the inner state store.
    pub fn state_store(&self) -> Arc<StateStore> {
        Arc::clone(&self.state_store)
    }

    /// Applies the block snapshot's storage changes to the local MPT state
    /// root store.
    pub fn on_committing(&self, block_index: u32, snapshot: &DataCache) -> bool {
        match self
            .state_store
            .apply_snapshot_changes(block_index, snapshot)
        {
            Ok(Some(root_hash)) => {
                debug!(
                    target: "neo.state_service",
                    block_index,
                    %root_hash,
                    "applied local state root"
                );
                true
            }
            Ok(None) => {
                debug!(
                    target: "neo.state_service",
                    block_index,
                    "state service has no MPT backend; skipping local state-root update"
                );
                true
            }
            Err(err) => {
                warn!(
                    target: "neo.state_service",
                    block_index,
                    %err,
                    "local state-root update failed"
                );
                false
            }
        }
    }

    /// Discards any state-root candidate whose block index falls in
    /// the supplied range (inclusive).
    pub fn on_reverting(&self, from_index: u32, to_index: u32) {
        for index in from_index..=to_index {
            if let Some(root) = self
                .state_store
                .get_state_root(crate::state_store::StateStoreLookup::ByBlockIndex(index))
            {
                self.state_store.discard(root.root_hash());
            }
        }
    }
}

impl CommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, block: &Block) {
        debug!(
            target: "neo.state_service",
            block_index = block.index(),
            "state service committed handler observed block"
        );
    }
}

impl CommittingHandler for StateServiceCommitHandlers {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        let _ = self.on_committing(block.index(), snapshot);
    }
}

#[cfg(test)]
#[path = "tests/commit_handlers.rs"]
mod tests;

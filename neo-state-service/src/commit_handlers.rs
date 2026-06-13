//! Block-commit handler pipeline for the state service.
//!
//! Wires the state-root calculation into the block persistence
//! pipeline:
//!
//! - On `Committed(block)` - asks the supplied [`StateRootCalculator`]
//!   to compute the state root for the block's storage change set and
//!   stages it as a candidate via [`StateStore::try_add_state_root`].
//! - On `Committing(block, snapshot, ...)` - drops any candidate
//!   state roots whose block index falls in the reverting range
//!   via [`StateStore::discard`].

use crate::state_root::StateRoot;
use crate::state_store::StateStore;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_primitives::UInt256;
use neo_storage::DataCache;
use std::any::Any;
use std::sync::Arc;
use tracing::{debug, warn};

/// Result of a state-root calculation, used to abstract the
/// pluggable MPT implementation.
pub trait StateRootCalculator: Send + Sync {
    /// Computes the state root for the block's storage change set.
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> Result<StateRoot, String>;
}

/// Default [`StateRootCalculator`] that hashes the snapshot's
/// changed-key set into a synthetic state root.
///
/// This is a stand-in for the real MPT (Merkle Patricia Trie); the
/// state-service's `verification` pipeline is responsible for
/// upgrading to the MPT once it is implemented.
pub struct SyntheticStateRootCalculator;

impl StateRootCalculator for SyntheticStateRootCalculator {
    fn compute(&self, block_index: u32, snapshot: &DataCache) -> Result<StateRoot, String> {
        let mut buf = Vec::new();
        for key in snapshot.get_change_set() {
            buf.extend_from_slice(&key.to_array());
        }
        let root_hash = UInt256::from(neo_crypto::Crypto::sha256(&buf));
        Ok(StateRoot::new_current(block_index, root_hash))
    }
}

/// Handlers for wiring state-root calculation into block persistence.
pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
    calculator: Arc<dyn StateRootCalculator>,
}

impl StateServiceCommitHandlers {
    /// Constructs a new pipeline backed by the supplied state store
    /// and calculator.
    pub fn new(state_store: Arc<StateStore>, calculator: Arc<dyn StateRootCalculator>) -> Self {
        Self {
            state_store,
            calculator,
        }
    }

    /// Returns a clone of the inner state store.
    pub fn state_store(&self) -> Arc<StateStore> {
        Arc::clone(&self.state_store)
    }

    /// Computes the state root for the given block and stages it as
    /// a candidate in the state store.
    pub fn on_committed(&self, block_index: u32, snapshot: &DataCache) -> bool {
        match self.calculator.compute(block_index, snapshot) {
            Ok(root) => {
                debug!(target: "neo.state_service", block_index, "computed state root");
                self.state_store.try_add_state_root(root)
            }
            Err(err) => {
                warn!(target: "neo.state_service", block_index, %err, "state root calculation failed");
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
        // The block's own index is the only state we need; the
        // snapshot used for state-root calculation lives in the
        // blockchain service, not on the event handler.
        let _ = block.index();
    }
}

impl CommittingHandler for StateServiceCommitHandlers {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        _block: &Block,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        // Real implementation would compute the state root here from
        // the snapshot. Left as a no-op for the in-memory stand-in.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::DataCache;

    #[test]
    fn committed_handler_stages_root() {
        let store = Arc::new(StateStore::new());
        let handlers = StateServiceCommitHandlers::new(
            Arc::clone(&store),
            Arc::new(SyntheticStateRootCalculator),
        );
        let snapshot = DataCache::new(false);
        assert!(handlers.on_committed(1, &snapshot));
        assert_eq!(store.candidate_count(), 1);
    }

    #[test]
    fn reverting_discards_root() {
        let store = Arc::new(StateStore::new());
        let handlers = StateServiceCommitHandlers::new(
            Arc::clone(&store),
            Arc::new(SyntheticStateRootCalculator),
        );
        let snapshot = DataCache::new(false);
        handlers.on_committed(5, &snapshot);
        assert_eq!(store.candidate_count(), 1);
        handlers.on_reverting(5, 5);
        assert_eq!(store.candidate_count(), 0);
    }
}

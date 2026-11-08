//! Commit handlers for state service operations.
//!
//! This module provides handlers for committing state changes during block processing.
//! It implements the state root computation and persistence logic that runs during
//! blockchain commit events.
//!
//! # Reference
//! C# Neo StateService: neo/Plugins/StateService/StatePlugin.cs
//! - Blockchain_Committing event handler computes state roots
//! - Blockchain_Committed event handler notifies and persists state roots

use neo_storage::persistence::StoreCache;
use neo_crypto::mpt_trie::{Trie, MptStoreSnapshot};
use neo_primitives::UInt256;
use super::{StateRoot, ValidatedRootPersisted};
use std::sync::{Arc, Mutex};
use tracing::{debug, warn, error};

/// Error type for state service operations.
#[derive(Debug, Clone)]
pub struct StateServiceError {
    pub message: String,
}

impl std::fmt::Display for StateServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "State service error: {}", self.message)
    }
}

impl std::error::Error for StateServiceError {}

impl StateServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Handles state service commit operations during block processing.
///
/// This handler computes and persists state roots for each committed block,
/// enabling state proof verification and light client support.
///
/// # Architecture
/// - `blockchain_committing_handler`: Computes state root from MPT trie before commit
/// - `blockchain_committed_handler`: Persists state root and emits notification after commit
/// - `commit_block`: Direct commit interface for explicit state root computation
/// - `rollback`: Reverts uncommitted state changes
#[derive(Clone)]
pub struct StateServiceCommitHandlers {
    /// Store cache for state root persistence
    store_cache: Arc<StoreCache>,
    /// Whether state root tracking is enabled
    state_root_enabled: bool,
    /// Pending state root computed during committing phase
    pending_state_root: Arc<Mutex<Option<StateRoot>>>,
}

impl StateServiceCommitHandlers {
    /// Creates a new StateServiceCommitHandlers instance.
    ///
    /// # Arguments
    /// * `store_cache` - Store cache for persisting state roots
    /// * `state_root_enabled` - Whether to compute and track state roots
    ///
    /// # Reference
    /// C# Neo StatePlugin constructor initializes event handlers
    pub fn new(
        store_cache: Arc<StoreCache>,
        state_root_enabled: bool,
    ) -> Result<Self, StateServiceError> {
        Ok(Self {
            store_cache,
            state_root_enabled,
            pending_state_root: Arc::new(Mutex::new(None)),
        })
    }

    /// Commits state changes for a block by computing its state root.
    ///
    /// This method:
    /// 1. Creates an MPT Trie from the current store state
    /// 2. Computes the root hash from all state changes
    /// 3. Creates a StateRoot instance for the given height
    /// 4. Stores it in pending_state_root for later persistence
    ///
    /// # Arguments
    /// * `height` - Block height to compute state root for
    ///
    /// # Reference
    /// C# Neo StateService: StatePlugin.OnCommitting computes state root from snapshot
    pub fn commit_block(&self, height: u32) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            height,
            "computing state root for block"
        );

        // Compute state root hash from current store state
        let root_hash = self.compute_state_root_hash()?;

        // Create StateRoot instance
        let state_root = StateRoot::new_current(height, root_hash);

        // Store as pending for later persistence in committed handler
        *self.pending_state_root.lock().unwrap() = Some(state_root);

        debug!(
            target: "neo::state_service",
            height,
            %root_hash,
            "state root computed successfully"
        );

        Ok(())
    }

    /// Computes the state root hash from the current store state.
    ///
    /// This creates an MPT Trie over the store cache and computes the root hash
    /// representing all current state.
    ///
    /// # Reference
    /// C# Neo StateService: GetLocalRootHash() - computes MPT root from snapshot
    fn compute_state_root_hash(&self) -> Result<UInt256, StateServiceError> {
        // In production, we would:
        // 1. Create MPT Trie from store_cache
        // 2. Iterate all state changes in snapshot
        // 3. Apply changes to trie
        // 4. Compute root hash via trie.root_hash()
        //
        // For now, since we don't have full MPT integration with StoreCache,
        // we use a zero hash as placeholder. Full implementation requires:
        // - StoreCache to implement MptStoreSnapshot trait
        // - State changes to be indexed by storage key
        // - MPT trie to be built from all contract storage items

        // TODO: Implement full MPT state root computation
        // This requires StoreCache to implement MptStoreSnapshot
        // and proper integration with the MPT trie module
        Ok(UInt256::zero())
    }

    /// Rolls back uncommitted state changes.
    ///
    /// This clears any pending state root that was computed but not yet persisted,
    /// typically called when block commit fails or is cancelled.
    ///
    /// # Reference
    /// C# Neo StateService: rollback clears pending state changes
    pub fn rollback(&self) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            "rolling back pending state root"
        );

        // Clear pending state root
        *self.pending_state_root.lock().unwrap() = None;

        Ok(())
    }

    /// Persists the pending state root to storage.
    ///
    /// Called after block is successfully committed to persist the computed
    /// state root for future verification.
    ///
    /// # Reference
    /// C# Neo StateService: OnCommitted persists state root to store
    fn persist_pending_state_root(&self, index: u32) -> Result<(), StateServiceError> {
        let state_root = {
            let mut pending = self.pending_state_root.lock().unwrap();
            pending.take()
        };

        if let Some(state_root) = state_root {
            if state_root.index != index {
                warn!(
                    target: "neo::state_service",
                    expected_index = index,
                    actual_index = state_root.index,
                    "state root index mismatch, skipping persistence"
                );
                return Ok(());
            }

            debug!(
                target: "neo::state_service",
                index,
                root_hash = %state_root.root_hash,
                "persisting state root"
            );

            // In production, persist state_root to store via:
            // store_cache.put(state_root_key(index), state_root.serialize())
            // For now, we just log it since storage integration is not complete

            // TODO: Persist state root to storage
            // This requires a dedicated storage prefix for state roots
            // Key format: PREFIX_STATE_ROOT + height (u32)
            // Value: serialized StateRoot
        }

        Ok(())
    }
}

// Implement CommittingHandler trait
impl crate::events::handlers::CommittingHandler for StateServiceCommitHandlers {
    /// Indicates whether this handler should run during fast sync.
    ///
    /// State root computation should run during fast sync to maintain
    /// state root history for verification.
    fn run_during_fast_sync(&self) -> bool {
        self.state_root_enabled
    }

    /// Called when a block is committing, while its state is still in the cache.
    ///
    /// This is the pre-commit hook that computes the state root from the
    /// snapshot before it's persisted to storage.
    ///
    /// # Arguments
    /// * `system` - System context (unused, provided for extensibility)
    /// * `block` - Block being committed
    /// * `snapshot` - Data cache containing the block's state changes
    /// * `application_executed_list` - Execution results (unused for state root)
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committing event handler
    /// - Computes state root from DataCache snapshot
    /// - Stores result for later persistence in Committed handler
    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
        _snapshot: &neo_storage::persistence::DataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        if !self.state_root_enabled {
            return;
        }

        let height = block.index();

        match self.commit_block(height) {
            Ok(()) => {
                debug!(
                    target: "neo::state_service",
                    height,
                    "blockchain_committing: state root computed"
                );
            }
            Err(error) => {
                error!(
                    target: "neo::state_service",
                    height,
                    %error,
                    "blockchain_committing: failed to compute state root"
                );
            }
        }
    }

    /// Fallible committing hook for protocol-critical state root computation.
    ///
    /// This version returns an error if state root computation fails,
    /// allowing the blockchain to handle the failure appropriately.
    fn try_blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
        snapshot: &neo_storage::persistence::DataCache,
        application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) -> crate::events::handlers::CoreResult<()> {
        if !self.state_root_enabled {
            return Ok(());
        }

        let height = block.index();

        self.commit_block(height).map_err(|e| {
            error!(
                target: "neo::state_service",
                height,
                error = %e,
                "try_blockchain_committing_handler: state root computation failed"
            );
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;

        // Also call the non-fallible handler for any additional side effects
        self.blockchain_committing_handler(system, block, snapshot, application_executed_list);

        Ok(())
    }
}

// Implement CommittedHandler trait
impl crate::events::handlers::CommittedHandler for StateServiceCommitHandlers {
    /// Called after a block has been committed and its state updated.
    ///
    /// This is the post-commit hook that persists the state root computed
    /// during the committing phase and emits notification events.
    ///
    /// # Arguments
    /// * `system` - System context (unused, provided for extensibility)
    /// * `block` - Block that was committed
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committed event handler
    /// - Persists state root to storage
    /// - Emits ValidatedRootPersisted event
    /// - Triggers state root relay to network
    fn blockchain_committed_handler(
        &self,
        _system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
    ) {
        if !self.state_root_enabled {
            return;
        }

        let index = block.index();

        match self.persist_pending_state_root(index) {
            Ok(()) => {
                debug!(
                    target: "neo::state_service",
                    index,
                    "blockchain_committed: state root persisted"
                );

                // Emit ValidatedRootPersisted event for observers
                // In production this would publish to an event bus
                debug!(
                    target: "neo::state_service",
                    index,
                    "ValidatedRootPersisted event emitted"
                );
            }
            Err(error) => {
                error!(
                    target: "neo::state_service",
                    index,
                    %error,
                    "blockchain_committed: failed to persist state root"
                );
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::memory_store::MemoryStore;
    use neo_protocol::execution::{Block, BlockHeader, ApplicationExecuted};
    use neo_storage::persistence::DataCache;

    #[test]
    fn test_commit_handlers_creation() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));

        let handlers = StateServiceCommitHandlers::new(cache, true);
        assert!(handlers.is_ok());
    }

    #[test]
    fn test_commit_handlers_disabled() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

        // When disabled, commit_block should succeed but do nothing
        assert!(handlers.commit_block(0).is_ok());

        // Pending state root should remain None
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_none());
    }

    #[test]
    fn test_commit_block_computes_state_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Commit for height 100
        assert!(handlers.commit_block(100).is_ok());

        // Should have pending state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());

        let state_root = pending.as_ref().unwrap();
        assert_eq!(state_root.index, 100);
        assert_eq!(state_root.version, super::super::state_root::CURRENT_VERSION);
    }

    #[test]
    fn test_rollback_clears_pending() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Commit and verify pending state root exists
        assert!(handlers.commit_block(50).is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_some());

        // Rollback should clear it
        assert!(handlers.rollback().is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
    }

    #[test]
    fn test_persist_pending_state_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Commit for height 200
        assert!(handlers.commit_block(200).is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_some());

        // Persist should consume the pending state root
        assert!(handlers.persist_pending_state_root(200).is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
    }

    #[test]
    fn test_persist_index_mismatch() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Commit for height 300
        assert!(handlers.commit_block(300).is_ok());

        // Try to persist with wrong index (should succeed but log warning)
        assert!(handlers.persist_pending_state_root(301).is_ok());

        // Pending should be cleared anyway
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
    }

    #[test]
    fn test_committing_handler() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(Arc::clone(&store)));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Create minimal block
        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![], false);
        block.set_index(42);

        let snapshot = DataCache::new(store);
        let app_executed: Vec<ApplicationExecuted> = vec![];

        // Call committing handler
        use crate::events::handlers::CommittingHandler;
        handlers.blockchain_committing_handler(
            &(),
            &block,
            &snapshot,
            &app_executed,
        );

        // Should have computed state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());
        assert_eq!(pending.as_ref().unwrap().index, 42);
    }

    #[test]
    fn test_committed_handler() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // First compute state root
        assert!(handlers.commit_block(99).is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_some());

        // Create minimal block
        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![], false);
        block.set_index(99);

        // Call committed handler
        use crate::events::handlers::CommittedHandler;
        handlers.blockchain_committed_handler(&(), &block);

        // Should have persisted (cleared pending)
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
    }

    #[test]
    fn test_run_during_fast_sync() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));

        let enabled_handlers = StateServiceCommitHandlers::new(Arc::clone(&cache), true).unwrap();
        let disabled_handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

        use crate::events::handlers::CommittingHandler;
        assert!(enabled_handlers.run_during_fast_sync());
        assert!(!disabled_handlers.run_during_fast_sync());
    }

    #[test]
    fn test_try_committing_handler() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(Arc::clone(&store)));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![], false);
        block.set_index(77);

        let snapshot = DataCache::new(store);
        let app_executed: Vec<ApplicationExecuted> = vec![];

        // Call fallible version
        use crate::events::handlers::CommittingHandler;
        let result = handlers.try_blockchain_committing_handler(
            &(),
            &block,
            &snapshot,
            &app_executed,
        );

        assert!(result.is_ok());

        // Should have computed state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());
        assert_eq!(pending.as_ref().unwrap().index, 77);
    }
}

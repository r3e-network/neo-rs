//! State-store service port owned by the state-service subsystem.
//!
//! Phase 1 S3 (Batch A): moved from `crate::services` so the port lives with
//! its vocabulary (`StateRoot`) ahead of the neo-state-service extraction.
//! `neo-system::services` re-exports it, preserving TypeId identity.

use super::StateRoot;
use neo_primitives::UInt256;
use std::sync::Arc;

/// State-store service trait backed by `StateRootStore`
/// (see `super::store`). The canonical ledger service trait
/// lives in `neo-ledger` (`neo_ledger::services::LedgerService`).
pub trait StateStoreService: Send + Sync {
    fn on_new_state_root(&self, state_root: StateRoot) -> bool;

    /// Stages local-root progress in the block's atomic precommit snapshot.
    fn stage_local_root(
        &self,
        state_root: &StateRoot,
        snapshot: &neo_storage::persistence::PersistenceDataCache,
    ) -> bool;

    /// Finishes a staged root after the block transaction commits successfully.
    fn on_local_root_committed(&self, state_root: &StateRoot) -> bool;

    /// Discards a staged marker when the surrounding block transaction rolls back.
    fn discard_staged_local_root(&self, index: u32);

    /// Get the current local root index
    fn local_root_index(&self) -> u64;

    /// Get the current validated root index
    fn validated_root_index(&self) -> u64;

    /// Set a fetcher for reference state roots
    fn set_reference_root_fetcher(
        &self,
        fetcher: Arc<dyn Fn(u32) -> Option<UInt256> + Send + Sync>,
    );

    /// Flush pending state changes to storage
    fn flush(&self);
}

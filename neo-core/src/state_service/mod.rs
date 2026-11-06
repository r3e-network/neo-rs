//! State Service Module
//!
//! This module re-exports the canonical state root computation and verification
//! service from the `neo-state-service` crate. The commit handlers that wired
//! state roots into block persistence moved to `neo-system::state_service`
//! (task W3-C2), and the `verification` actor moved to
//! `neo-system::state_verification` (task 208).

// ---------------------------------------------------------------------------
// Re-exported modules from neo-state-service (always available)
// ---------------------------------------------------------------------------
pub use neo_state_service::async_state_root;
pub use neo_state_service::keys;
pub use neo_state_service::message_type;
/// Metrics counters for state root ingestion from the network.
pub use neo_state_service::metrics;
pub use neo_state_service::read_only_store;
pub use neo_state_service::root_cache;
pub use neo_state_service::state_root;
pub use neo_state_service::state_store;
pub use neo_state_service::tiered_storage;
pub use neo_state_service::vote;

// ---------------------------------------------------------------------------
// Re-exported modules from neo-state-service (runtime feature gate)
// ---------------------------------------------------------------------------
#[cfg(feature = "runtime")]
pub use neo_state_service::conflict_detector;
#[cfg(feature = "runtime")]
pub use neo_state_service::optimistic_executor;
#[cfg(feature = "runtime")]
pub use neo_state_service::parallel_execution_integration;
#[cfg(feature = "runtime")]
pub use neo_state_service::prefetch_pipeline;

// ---------------------------------------------------------------------------
// Re-exported modules from neo-state-service (prefetch feature gate)
// ---------------------------------------------------------------------------
#[cfg(feature = "prefetch")]
pub use neo_state_service::prefetch_background;
#[cfg(feature = "prefetch")]
pub use neo_state_service::prefetch_worker;

// ---------------------------------------------------------------------------
// Local modules
// ---------------------------------------------------------------------------
// `commit_handlers` and `test_support` moved to
// `neo-system::state_service` (task W3-C2) together with the blockchain actor
// and the `NeoSystem` handler registry that drive them.

// ---------------------------------------------------------------------------
// Flat re-exports (public API surface)
// ---------------------------------------------------------------------------
pub use neo_state_service::STATE_SERVICE_CATEGORY;
pub use neo_state_service::ValidatedRootPersisted;
pub use neo_state_service::Vote;
pub use neo_state_service::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
};
pub use neo_state_service::{Keys, MessageType, StateRootIngestStats};
pub use neo_state_service::{StateRoot, StateRootDivergence, StateStore, StateStoreTransaction};

// ---------------------------------------------------------------------------
// Trait impls bridging neo-core's local WitnessData / VerifiableExt traits to
// the re-exported StateRoot type from neo-state-service.
// ---------------------------------------------------------------------------
use crate::persistence::DataCache;
use crate::smart_contract::native::Role;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::role_management::RoleManagement;
use crate::witness::Witness;
use crate::{UInt160, VerifiableExt, WitnessData};

impl WitnessData for StateRoot {
    fn witnesses(&self) -> Vec<&Witness> {
        self.witness.as_ref().map(|w| vec![w]).unwrap_or_default()
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.witness.as_mut().map(|w| vec![w]).unwrap_or_default()
    }
}

impl VerifiableExt for StateRoot {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        let validators = RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::StateValidator, self.index)
            .unwrap_or_default();
        if validators.is_empty() {
            return Vec::new();
        }
        vec![NativeHelpers::get_bft_address(&validators)]
    }
}

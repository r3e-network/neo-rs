//! Service trait definitions for the state-service subsystem.
//!
//! Distributed from `neo-core::services::traits` / `neo-core::services`. The
//! `StateStoreService` trait and its canonical implementation for [`StateStore`]
//! live together here because `StateStore` is owned by this crate.

use crate::state_store::StateStore;

/// State store operations required by RPC/health checks.
///
/// Provides access to state root indices for monitoring sync progress.
pub trait StateStoreService: Send + Sync {
    /// Latest local state root index.
    fn local_root_index(&self) -> Option<u32>;

    /// Latest validated state root index.
    fn validated_root_index(&self) -> Option<u32>;
}

impl StateStoreService for StateStore {
    fn local_root_index(&self) -> Option<u32> {
        StateStore::local_root_index(self)
    }

    fn validated_root_index(&self) -> Option<u32> {
        StateStore::validated_root_index(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStateStore {
        local: Option<u32>,
        validated: Option<u32>,
    }

    impl StateStoreService for MockStateStore {
        fn local_root_index(&self) -> Option<u32> {
            self.local
        }

        fn validated_root_index(&self) -> Option<u32> {
            self.validated
        }
    }

    #[test]
    fn mock_state_store_service_works() {
        let store = MockStateStore {
            local: Some(10),
            validated: Some(8),
        };
        assert_eq!(store.local_root_index(), Some(10));
        assert_eq!(store.validated_root_index(), Some(8));
    }

    #[test]
    fn trait_is_object_safe() {
        fn _accepts_state_store(_: &dyn StateStoreService) {}
    }
}

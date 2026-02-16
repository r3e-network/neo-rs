use super::*;
use crate::persistence::store_cache::StoreCache;

/// Verifies state roots using the designated validator set.
#[derive(Clone)]
pub struct StateRootVerifier {
    settings: Arc<ProtocolSettings>,
    snapshot_provider: Arc<dyn Fn() -> DataCache + Send + Sync>,
}

impl StateRootVerifier {
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot_provider: Arc<dyn Fn() -> DataCache + Send + Sync>,
    ) -> Self {
        Self {
            settings,
            snapshot_provider,
        }
    }

    pub(super) fn verify(&self, state_root: &StateRoot) -> bool {
        let snapshot = (self.snapshot_provider)();
        state_root.verify(&self.settings, &snapshot)
    }

    /// Builds a verifier that reads state from the provided store using a read-only cache.
    pub fn from_store(store: Arc<dyn IStore>, settings: Arc<ProtocolSettings>) -> Self {
        Self::new(
            settings,
            Arc::new(move || {
                // Fresh read-only view for each verification to avoid mutability concerns.
                let cache = StoreCache::new_from_store(store.clone(), true);
                cache.data_cache().clone_cache()
            }),
        )
    }
}

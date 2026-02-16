//! Storage initialization and ledger hydration helpers for `NeoSystem`.

use std::sync::Arc;

use crate::ledger::{HeaderCache, LedgerContext};
use crate::persistence::{StoreCache, i_store::IStore};
use crate::protocol_settings::ProtocolSettings;
use crate::state_service::{
    StateStore,
    state_store::{SnapshotBackedStateStoreBackend, StateRootVerifier, StateServiceSettings},
};

/// Initializes store, cache, and state store from a provider and optional path.
pub(crate) fn init_store(
    store_provider: Arc<dyn crate::persistence::i_store_provider::IStoreProvider>,
    storage_path: Option<String>,
    settings: Arc<ProtocolSettings>,
    state_service_settings: Option<StateServiceSettings>,
) -> crate::error::CoreResult<(Arc<dyn IStore>, StoreCache, Arc<StateStore>)> {
    let store = store_provider.get_store(storage_path.as_deref().unwrap_or(""))?;
    let store_cache_for_hydration = StoreCache::new_from_store(store.clone(), true);

    let state_store = if let Some(state_settings) = state_service_settings {
        // Use a dedicated store for state roots / MPT nodes (mirrors C# StateService plugin).
        // Witness verification must still consult the *blockchain* store for RoleManagement lookups.
        let state_db = store_provider.get_store(&state_settings.path)?;
        let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
        let verifier = StateRootVerifier::from_store(store.clone(), settings);
        Arc::new(StateStore::new_with_verifier(
            backend,
            state_settings,
            Some(verifier),
        ))
    } else {
        // Disabled by default: keep an in-memory store instance but do not expose it via the
        // NeoSystemContext unless explicitly enabled.
        Arc::new(StateStore::new_in_memory())
    };

    Ok((store, store_cache_for_hydration, state_store))
}

/// Hydrates ledger and header cache from persistent storage.
pub(crate) fn hydrate_ledger(
    store_cache_for_hydration: &StoreCache,
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
) {
    crate::neo_system::NeoSystemContext::hydrate_ledger_from_store(
        store_cache_for_hydration,
        ledger,
        header_cache,
    );
}

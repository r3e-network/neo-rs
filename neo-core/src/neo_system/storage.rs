//! Storage initialization and ledger hydration helpers for `NeoSystem`.

use std::sync::Arc;

use crate::ledger::{HeaderCache, LedgerContext};
use crate::persistence::{i_store::IStore, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::state_service::{state_store::StateServiceSettings, StateStore};

/// Initializes store, cache, and state store from a provider and optional path.
pub(crate) fn init_store(
    store_provider: Arc<dyn crate::persistence::i_store_provider::IStoreProvider>,
    storage_path: Option<String>,
    settings: Arc<ProtocolSettings>,
) -> crate::error::CoreResult<(Arc<dyn IStore>, StoreCache, Arc<StateStore>)> {
    let store = store_provider.get_store(storage_path.as_deref().unwrap_or(""))?;
    let store_cache_for_hydration = StoreCache::new_from_store(store.clone(), true);
    let state_store = Arc::new(StateStore::new_from_store(
        store.clone(),
        StateServiceSettings::default(),
        settings,
    ));
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

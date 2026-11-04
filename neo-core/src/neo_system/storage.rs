//! Storage initialization and ledger hydration helpers for `NeoSystem`.

use std::sync::Arc;

use crate::ledger::{HeaderCache, LedgerContext};
use crate::persistence::{StoreCache, store::Store};
use crate::protocol_settings::ProtocolSettings;
use crate::state_service::{
    StateStore,
    state_store::{SnapshotBackedStateStoreBackend, StateRootVerifier, StateServiceSettings},
};

/// Initializes store, cache, and state store from a provider and optional path.
pub(crate) fn init_store(
    store_provider: Arc<dyn crate::persistence::store_provider::StoreProvider>,
    storage_path: Option<String>,
    settings: Arc<ProtocolSettings>,
    state_service_settings: Option<StateServiceSettings>,
) -> crate::error::CoreResult<(Arc<dyn Store>, StoreCache, Arc<StateStore>)> {
    let store = store_provider.get_store(storage_path.as_deref().unwrap_or(""))?;
    let store_cache_for_hydration = StoreCache::new_from_store(store.clone(), true);
    let chain_network = settings.network;

    let state_store = if let Some(state_settings) = state_service_settings {
        // Use a dedicated store for state roots / MPT nodes (mirrors C# StateService plugin).
        // Witness verification must still consult the *blockchain* store for RoleManagement lookups.
        let state_settings_network = state_settings.network;
        let state_db = store_provider.get_store(&state_settings.path)?;
        let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
        let verifier = StateRootVerifier::from_store(store.clone(), settings);
        let mut ss = StateStore::new_with_verifier(backend, state_settings, Some(verifier));
        // Load reference state roots for validation when available or required.
        let default_ref_path = "data/reference_stateroots.jsonl";
        let ref_path_override = std::env::var("NEO_REFERENCE_ROOTS_PATH").ok();
        let require_reference_roots =
            std::env::var("NEO_REQUIRE_REFERENCE_ROOTS").as_deref() == Ok("1");
        ss.set_reference_roots_required(require_reference_roots);

        // Reference state roots are network-specific. The ambient default file
        // may only be adopted when the state service is configured for this
        // chain's network — mirroring the startup config's network-consistency
        // gate — so a programmatically built system (unit tests, tools) pairing
        // fallback protocol settings with unrelated StateServiceSettings cannot
        // silently import MainNet goldens found in the working directory. An
        // explicit NEO_REFERENCE_ROOTS_PATH is operator intent and is honored
        // regardless of the network mismatch.
        let ambient_eligible =
            ref_path_override.is_some() || state_settings_network == chain_network;
        // Observability only: when the state service is configured for a different
        // network and reference roots are not required, the mismatch previously
        // passed silently (ambient roots are simply not adopted). Surface it as a
        // startup warning; behavior is unchanged.
        if let Some(warning) = network_mismatch_reference_roots_warning(
            state_settings_network,
            chain_network,
            require_reference_roots,
        ) {
            tracing::warn!(target: "neo", "{warning}");
        }
        let target_ref_path = ref_path_override.as_deref().unwrap_or(default_ref_path);

        if ambient_eligible && std::path::Path::new(target_ref_path).exists() {
            ss.load_reference_roots(target_ref_path).map_err(|e| {
                crate::error::CoreError::system(format!(
                    "failed to load reference roots from '{target_ref_path}': {e}"
                ))
            })?;
        } else if ref_path_override.is_some()
            || (require_reference_roots && std::env::var_os("NEO_REFERENCE_ROOTS_RPC").is_none())
        {
            let reason = if ambient_eligible {
                format!("reference roots file required but not found: '{target_ref_path}'")
            } else {
                format!(
                    "reference roots required but the state service network {state_settings_network:#010x} \
                     does not match the chain network {chain_network:#010x}; set NEO_REFERENCE_ROOTS_PATH or NEO_REFERENCE_ROOTS_RPC"
                )
            };
            return Err(crate::error::CoreError::system(reason));
        }
        let ss = Arc::new(ss);
        // Populate the trie with current blockchain storage if the trie is empty.
        // Without this, only block deltas would be in the trie, producing wrong roots.
        ss.initialize_trie_from_store(&store);
        ss
    } else {
        // Disabled by default: keep an in-memory store instance but do not expose it via the
        // NeoSystemContext unless explicitly enabled.
        Arc::new(StateStore::new_in_memory())
    };

    Ok((store, store_cache_for_hydration, state_store))
}

/// Builds the startup warning emitted when the state service is configured for a
/// network different from the chain network while reference state roots are *not*
/// required. Returns `None` when the networks match, or when reference roots are
/// required (the startup gate then reports a hard error instead of a warning).
/// Kept pure so the exact condition is unit-testable; `init_store` emits the
/// resulting message through the crate's logging facility.
fn network_mismatch_reference_roots_warning(
    state_settings_network: u32,
    chain_network: u32,
    require_reference_roots: bool,
) -> Option<String> {
    (state_settings_network != chain_network && !require_reference_roots).then(|| {
        format!(
            "state service network {state_settings_network:#010x} does not match the chain \
             network {chain_network:#010x}; ambient reference state roots are not adopted for a \
             mismatched network (set NEO_REFERENCE_ROOTS_PATH to load an explicit file)"
        )
    })
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

#[cfg(test)]
mod tests {
    use super::network_mismatch_reference_roots_warning;

    #[test]
    fn network_mismatch_without_required_reference_roots_warns() {
        let warning = network_mismatch_reference_roots_warning(519_508, 860_833, false).unwrap();
        // The emitted warning names both networks and points at the remediation.
        assert!(warning.contains("does not match the chain network"));
        assert!(warning.contains("NEO_REFERENCE_ROOTS_PATH"));
        assert!(warning.contains("0x0007ed54"));
        assert!(warning.contains("0x000d22a1"));
    }

    #[test]
    fn matching_networks_do_not_warn() {
        assert!(network_mismatch_reference_roots_warning(519_508, 519_508, false).is_none());
    }

    #[test]
    fn mismatch_with_required_reference_roots_does_not_warn() {
        // Required roots are handled by the hard-error gate, not the warning.
        assert!(network_mismatch_reference_roots_warning(519_508, 860_833, true).is_none());
    }
}

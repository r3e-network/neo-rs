//! StateService MPT store and commit-handler construction.

use std::sync::Arc;

use anyhow::Context;
use neo_storage::persistence::providers::{MemoryStore, RuntimeStore};
use tracing::info;

use crate::node::config::{NodeConfig, network_scoped_path};
use crate::node::inventory_relay::FAST_SYNC_BURST_CAPACITY;

use super::store::{ServiceStore, open_service_store_with_storage_config};

pub(super) struct StateServiceRuntime {
    pub(super) state_store: Option<Arc<neo_state_service::StateStore<RuntimeStore>>>,
    pub(super) state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>>,
    pub(super) durable_store: Option<ServiceStore>,
}

pub(super) fn build_state_service_runtime(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
    service_fast_sync: bool,
    canonical_store: &Arc<RuntimeStore>,
) -> anyhow::Result<StateServiceRuntime> {
    let state_service_fast_sync = service_fast_sync && config.state_service.track_during_catchup;
    let (state_store, durable_store, coordinated) = build_state_store(
        config,
        network,
        storage_provider,
        state_service_fast_sync,
        canonical_store,
    )?;
    let state_service = match (state_store.as_ref(), coordinated) {
        (Some(state_store), true) => Some(Arc::new(
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(state_store),
            )
            .map_err(anyhow::Error::msg)
            .context("constructing coordinated StateService commit handlers")?,
        )),
        (Some(state_store), false) if state_service_fast_sync => {
            let handlers =
                neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_async_with_capacity(
                    Arc::clone(state_store),
                    FAST_SYNC_BURST_CAPACITY,
                )
                .context("spawning StateService MPT worker")?;
            Some(Arc::new(handlers))
        }
        (Some(state_store), false) => Some(Arc::new(
            neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
                state_store,
            )),
        )),
        (None, _) => None,
    };

    Ok(StateServiceRuntime {
        state_store,
        state_service,
        durable_store,
    })
}

fn build_state_store(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
    fast_sync: bool,
    canonical_store: &Arc<RuntimeStore>,
) -> anyhow::Result<(
    Option<Arc<neo_state_service::StateStore<RuntimeStore>>>,
    Option<ServiceStore>,
    bool,
)> {
    if !config.state_service.enabled {
        return Ok((None, None, false));
    }

    let mut durable_store = None;
    let coordinated =
        storage_provider.eq_ignore_ascii_case("mdbx") && canonical_store.as_mdbx().is_some();
    let state_store = if coordinated {
        let backing = Arc::new(
            canonical_store
                .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
                .context("opening StateService MDBX namespace")?,
        );
        durable_store = Some(Arc::clone(&backing));
        Arc::new(
            neo_state_service::StateStore::with_mpt_store(config.state_service.full_state, backing)
                .context("opening coordinated StateService MPT store")?,
        )
    } else if let Some(path) = &config.state_service.path {
        let backing = open_service_store_with_storage_config(
            "StateService",
            storage_provider,
            &config.storage,
            path,
            network,
            fast_sync,
        )?;
        durable_store = Some(Arc::clone(&backing));
        Arc::new(
            neo_state_service::StateStore::with_mpt_store(config.state_service.full_state, backing)
                .with_context(|| {
                    format!(
                        "opening StateService MPT store at {}",
                        network_scoped_path(path, network).display()
                    )
                })?,
        )
    } else {
        let backing: ServiceStore = Arc::new(RuntimeStore::Memory(MemoryStore::new()));
        Arc::new(
            neo_state_service::StateStore::with_mpt_store(config.state_service.full_state, backing)
                .context("opening in-memory StateService MPT store")?,
        )
    };
    info!(
        target: "neo",
        full_state = config.state_service.full_state,
        coordinated,
        "state service MPT store enabled"
    );
    Ok((Some(state_store), durable_store, coordinated))
}

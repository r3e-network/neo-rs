//! StateService MPT store and commit-handler construction.

use std::sync::Arc;

use anyhow::Context;
use tracing::info;

use crate::node::config::{NodeConfig, network_scoped_path};
use crate::node::inventory_relay::FAST_SYNC_BURST_CAPACITY;

use super::store::{ServiceStore, open_service_store_with_storage_config};

pub(super) struct StateServiceRuntime {
    pub(super) state_store: Option<Arc<neo_state_service::StateStore>>,
    pub(super) state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
    pub(super) durable_store: Option<ServiceStore>,
}

pub(super) fn build_state_service_runtime(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
    service_fast_sync: bool,
) -> anyhow::Result<StateServiceRuntime> {
    let state_service_fast_sync = service_fast_sync && config.state_service.track_during_catchup;
    let (state_store, durable_store) =
        build_state_store(config, network, storage_provider, state_service_fast_sync)?;
    let state_service = state_store.as_ref().map(|state_store| {
        let handlers = if state_service_fast_sync {
            neo_state_service::commit_handlers::StateServiceCommitHandlers::new_async_with_capacity(
                Arc::clone(state_store),
                FAST_SYNC_BURST_CAPACITY,
            )
        } else {
            neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
                state_store,
            ))
        };
        Arc::new(handlers)
    });

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
) -> anyhow::Result<(
    Option<Arc<neo_state_service::StateStore>>,
    Option<ServiceStore>,
)> {
    if !config.state_service.enabled {
        return Ok((None, None));
    }

    let mut durable_store = None;
    let state_store = if let Some(path) = &config.state_service.path {
        let backing = open_service_store_with_storage_config(
            "StateService",
            storage_provider,
            &config.storage,
            path,
            network,
            fast_sync,
        )?;
        durable_store = Some(Arc::clone(&backing) as ServiceStore);
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
        Arc::new(neo_state_service::StateStore::with_mpt(
            config.state_service.full_state,
        ))
    };
    info!(
        target: "neo",
        full_state = config.state_service.full_state,
        "state service MPT store enabled"
    );
    Ok((Some(state_store), durable_store))
}

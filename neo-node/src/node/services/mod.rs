//! # neo-node::node::services
//!
//! Auxiliary service startup and handles used by the daemon.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `store`: service-store opening and fast-sync backend mode.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use tracing::info;

use super::config::{NodeConfig, network_scoped_path, service_store_provider};
use super::inventory_relay::FAST_SYNC_BURST_CAPACITY;

mod store;

use store::ServiceStore;
pub(in crate::node) use store::open_service_store_with_storage_config;

type TokensTrackerRuntime = (
    neo_rpc::plugins::tokens_tracker::TokensTrackerSettings,
    ServiceStore,
);

pub(super) struct OperationalServices {
    pub(super) state_store: Option<Arc<neo_state_service::StateStore>>,
    pub(super) state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
    pub(super) indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    pub(super) application_logs_service:
        Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    pub(super) tokens_tracker_service:
        Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService>>,
    pub(super) tokens_tracker_runtime: Option<TokensTrackerRuntime>,
    pub(super) durable_stores: Vec<ServiceStore>,
}

pub(super) fn build_operational_services(
    config: &NodeConfig,
    network: u32,
    enable_local_replay_services: bool,
    service_fast_sync: bool,
) -> anyhow::Result<OperationalServices> {
    if !enable_local_replay_services {
        info!(
            target: "neo::remote_ledger",
            "local replay-derived services disabled for remote-ledger mode"
        );
        return Ok(OperationalServices {
            state_store: None,
            state_service: None,
            indexer_service: None,
            application_logs_service: None,
            tokens_tracker_service: None,
            tokens_tracker_runtime: None,
            durable_stores: Vec::new(),
        });
    }

    let state_service_fast_sync = service_fast_sync && config.state_service.track_during_catchup;
    let storage_provider = service_store_provider(config)?;
    let (state_store, state_service_store) =
        build_state_store(config, network, &storage_provider, state_service_fast_sync)?;
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
    let indexer_service = build_indexer_service(config, network, &storage_provider)?;
    let application_logs_service =
        build_application_logs_service(config, network, &storage_provider)?;
    let (tokens_tracker_service, tokens_tracker_runtime) =
        build_tokens_tracker_services(config, network, &storage_provider)?;
    let durable_stores = state_service_store.into_iter().collect();

    Ok(OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
        durable_stores,
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

fn build_indexer_service(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
) -> anyhow::Result<Option<Arc<neo_indexer::IndexerService>>> {
    if !config.indexer.enabled {
        return Ok(None);
    }

    let service = if let Some(path) = &config.indexer.store_path {
        let path = network_scoped_path(path, network);
        let store = open_service_store_with_storage_config(
            "NeoIndexer",
            storage_provider,
            &config.storage,
            &path,
            network,
            false,
        )?;
        Arc::new(
            neo_indexer::IndexerService::open_store_with_path(store, Some(path.clone()))
                .with_context(|| format!("opening indexer service store at {}", path.display()))?,
        )
    } else if let Some(path) = &config.indexer.path {
        let path = network_scoped_path(path, network);
        Arc::new(
            neo_indexer::IndexerService::open(&path)
                .with_context(|| format!("opening indexer snapshot at {}", path.display()))?,
        )
    } else {
        Arc::new(neo_indexer::IndexerService::new())
    };
    info!(
        target: "neo::indexer",
        backfill_on_startup = config.indexer.backfill_on_startup,
        persistence_mode = service.persistence_mode(),
        snapshot_path = ?service.snapshot_path(),
        store_path = ?service.store_path(),
        "indexer service enabled"
    );
    Ok(Some(service))
}

fn build_application_logs_service(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
) -> anyhow::Result<Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>> {
    if !config.application_logs.enabled {
        return Ok(None);
    }

    let logs_settings = config.application_logs.settings(network);
    let logs_store = open_service_store_with_storage_config(
        "ApplicationLogs",
        storage_provider,
        &config.storage,
        Path::new(&logs_settings.path),
        network,
        false,
    )?;
    let service = Arc::new(neo_rpc::application_logs::ApplicationLogsService::new(
        logs_settings.clone(),
        logs_store,
    ));
    info!(
        target: "neo::application_logs",
        path = %logs_settings.path,
        debug = logs_settings.debug,
        "application logs service enabled"
    );
    Ok(Some(service))
}

fn build_tokens_tracker_services(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
) -> anyhow::Result<(
    Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService>>,
    Option<TokensTrackerRuntime>,
)> {
    if !config.tokens_tracker.enabled {
        return Ok((None, None));
    }

    let tracker_settings = config.tokens_tracker.settings(network);
    let tracker_store = open_service_store_with_storage_config(
        "TokensTracker",
        storage_provider,
        &config.storage,
        Path::new(&tracker_settings.db_path),
        network,
        false,
    )?;
    let service = Arc::new(neo_rpc::plugins::tokens_tracker::TokensTrackerService::new(
        tracker_settings.clone(),
        Arc::clone(&tracker_store),
    ));
    info!(
        target: "neo::tokens_tracker",
        path = %tracker_settings.db_path,
        track_history = tracker_settings.track_history,
        max_results = tracker_settings.max_results,
        enabled_trackers = ?tracker_settings.enabled_trackers,
        "tokens tracker service enabled"
    );
    Ok((Some(service), Some((tracker_settings, tracker_store))))
}

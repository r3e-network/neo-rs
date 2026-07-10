//! Read-side service construction for replay-derived query surfaces.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use neo_storage::persistence::providers::RuntimeStore;
use tracing::info;

use crate::node::config::{NodeConfig, network_scoped_path};

use super::store::{ServiceStore, open_service_store_with_storage_config};

pub(super) type TokensTrackerRuntime = (
    neo_rpc::plugins::tokens_tracker::TokensTrackerSettings,
    ServiceStore,
);

pub(super) struct ReadSideServices {
    pub(super) indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    pub(super) application_logs_service:
        Option<Arc<neo_rpc::application_logs::ApplicationLogsService<RuntimeStore>>>,
    pub(super) tokens_tracker_service:
        Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<RuntimeStore>>>,
    pub(super) tokens_tracker_runtime: Option<TokensTrackerRuntime>,
}

pub(super) fn build_read_side_services(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
) -> anyhow::Result<ReadSideServices> {
    let indexer_service = build_indexer_service(config, network, storage_provider)?;
    let application_logs_service =
        build_application_logs_service(config, network, storage_provider)?;
    let (tokens_tracker_service, tokens_tracker_runtime) =
        build_tokens_tracker_services(config, network, storage_provider)?;

    Ok(ReadSideServices {
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
    })
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
) -> anyhow::Result<Option<Arc<neo_rpc::application_logs::ApplicationLogsService<RuntimeStore>>>> {
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
    Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<RuntimeStore>>>,
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

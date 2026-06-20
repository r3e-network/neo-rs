use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tracing::info;

use super::config::{NodeConfig, network_scoped_path};

type ServiceStore = Arc<dyn neo_storage::persistence::store::Store>;
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
}

pub(super) fn build_operational_services(
    config: &NodeConfig,
    network: u32,
) -> anyhow::Result<OperationalServices> {
    let state_store = build_state_store(config, network)?;
    let state_service = state_store.as_ref().map(|state_store| {
        Arc::new(
            neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
                state_store,
            )),
        )
    });
    let indexer_service = build_indexer_service(config, network)?;
    let application_logs_service = build_application_logs_service(config, network)?;
    let (tokens_tracker_service, tokens_tracker_runtime) =
        build_tokens_tracker_services(config, network)?;

    Ok(OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
    })
}

fn build_state_store(
    config: &NodeConfig,
    network: u32,
) -> anyhow::Result<Option<Arc<neo_state_service::StateStore>>> {
    if !config.state_service.enabled {
        return Ok(None);
    }

    let state_store = if let Some(path) = &config.state_service.path {
        let path = network_scoped_path(path, network);
        let backing = open_service_store("StateService", &path)?;
        Arc::new(
            neo_state_service::StateStore::with_mpt_store(config.state_service.full_state, backing)
                .with_context(|| format!("opening StateService MPT store at {}", path.display()))?,
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
    Ok(Some(state_store))
}

fn build_indexer_service(
    config: &NodeConfig,
    network: u32,
) -> anyhow::Result<Option<Arc<neo_indexer::IndexerService>>> {
    if !config.indexer.enabled {
        return Ok(None);
    }

    let service = if let Some(path) = &config.indexer.store_path {
        let path = network_scoped_path(path, network);
        let store = open_service_store("NeoIndexer", &path)?;
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
) -> anyhow::Result<Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>> {
    if !config.application_logs.enabled {
        return Ok(None);
    }

    let logs_settings = config.application_logs.settings(network);
    let logs_store = open_service_store("ApplicationLogs", Path::new(&logs_settings.path))?;
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
) -> anyhow::Result<(
    Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService>>,
    Option<TokensTrackerRuntime>,
)> {
    if !config.tokens_tracker.enabled {
        return Ok((None, None));
    }

    let tracker_settings = config.tokens_tracker.settings(network);
    let tracker_store = open_service_store("TokensTracker", Path::new(&tracker_settings.db_path))?;
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

fn open_service_store(service_name: &'static str, path: &Path) -> anyhow::Result<ServiceStore> {
    use neo_storage::persistence::StoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::rocksdb::RocksDBStoreProvider;

    info!(target: "neo", service = service_name, path = %path.display(), "opening service RocksDB store");
    let cfg = StorageConfig {
        path: PathBuf::from(path),
        ..Default::default()
    };
    RocksDBStoreProvider::new(cfg)
        .get_store("")
        .map_err(|err| anyhow::anyhow!("failed to open {service_name} RocksDB store: {err}"))
}

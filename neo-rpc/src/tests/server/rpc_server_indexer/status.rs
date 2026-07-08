use std::sync::Arc;

use crate::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_indexer::IndexerService;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;

use super::super::RpcServerIndexer;
use super::support::{block, find_handler};

#[test]
fn indexer_status_ledger_height_uses_indexer_provider_boundary() {
    let status = include_str!("../../../server/rpc_server_indexer/status.rs");
    let height_start = status
        .find("fn ledger_height")
        .expect("ledger_height exists");
    let height_reader = &status[height_start..];

    assert!(
        height_reader.contains("NativeIndexerLedgerProviderFactory"),
        "indexer status ledger height reads must route through the local indexer provider factory"
    );
    assert!(
        !height_reader.contains("StorageLedgerProviderFactory"),
        "indexer status response assembly should not construct raw ledger providers directly"
    );
    assert!(
        !height_reader.contains("LedgerContract::new()"),
        "indexer status must not construct native LedgerContract directly"
    );

    let provider = include_str!("../../../server/rpc_server_indexer/ledger_provider.rs");
    assert!(provider.contains("trait IndexerLedgerProvider"));
    assert!(provider.contains("trait IndexerLedgerProviderFactory"));
    assert!(provider.contains("struct NativeIndexerLedgerProviderFactory"));
    assert!(
        provider.contains("ledger_queries::current_index"),
        "indexer ledger provider should use the shared ledger-query boundary"
    );
    assert!(
        !provider.contains("StorageLedgerProviderFactory"),
        "indexer ledger provider should not duplicate raw ledger provider construction"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_status_does_not_sync_when_indexer_is_ahead_of_ledger() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let service = Arc::new(IndexerService::new());
    service
        .index_block(&block(7, Vec::new()))
        .expect("index block ahead of ledger");
    system.register_service(service);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let status =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[]).expect("status");

    assert_eq!(status["indexedheight"].as_u64(), Some(7));
    assert_eq!(status["ledgerheight"].as_u64(), Some(0));
    assert_eq!(status["blocksbehind"].as_u64(), Some(0));
    assert_eq!(status["synced"].as_bool(), Some(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_status_reports_sync_lag_and_application_logs_availability() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    system.register_service(Arc::new(IndexerService::new()));

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.path = "ApplicationLogs_004F454E".to_string();
    system.register_service(Arc::new(ApplicationLogsService::new(
        logs_settings.clone(),
        Arc::new(MemoryStore::new()),
    )));

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let status =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[]).expect("status");

    assert!(status["indexedheight"].is_null());
    assert_eq!(status["ledgerheight"].as_u64(), Some(0));
    assert_eq!(status["blocksbehind"].as_u64(), Some(1));
    assert_eq!(status["synced"].as_bool(), Some(false));
    assert_eq!(status["applicationlogs"]["enabled"].as_bool(), Some(true));
    assert_eq!(
        status["applicationlogs"]["notificationrecovery"].as_bool(),
        Some(true)
    );
    assert_eq!(
        status["applicationlogs"]["path"].as_str(),
        Some(logs_settings.path.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_status_reports_persistent_snapshot_path() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let path = std::env::temp_dir().join(format!(
        "neo-rpc-indexer-status-{}-{}.json",
        std::process::id(),
        line!()
    ));
    let service = Arc::new(IndexerService::open(&path).expect("persistent indexer"));
    system.register_service(service);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let status =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[]).expect("status");

    assert_eq!(status["persistent"].as_bool(), Some(true));
    assert_eq!(status["persistencemode"].as_str(), Some("json-snapshot"));
    assert_eq!(
        status["snapshotpath"].as_str(),
        Some(path.display().to_string().as_str())
    );
    assert!(status["storepath"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_status_reports_service_store_path() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let path = std::path::PathBuf::from("Indexer_004F454E");
    let service = Arc::new(
        IndexerService::open_store_with_path(store, Some(path.clone())).expect("store indexer"),
    );
    system.register_service(service);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let status =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[]).expect("status");

    assert_eq!(status["persistent"].as_bool(), Some(true));
    assert_eq!(status["persistencemode"].as_str(), Some("service-store"));
    assert!(status["snapshotpath"].is_null());
    assert_eq!(
        status["storepath"].as_str(),
        Some(path.display().to_string().as_str())
    );
}

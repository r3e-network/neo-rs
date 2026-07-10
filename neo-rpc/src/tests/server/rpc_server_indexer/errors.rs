use std::sync::Arc;

use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_indexer::IndexerService;
use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use serde_json::Value;

use super::super::RpcServerIndexer;
use super::support::{block, corrupt_block_by_height_record, find_handler};

#[tokio::test(flavor = "multi_thread")]
async fn indexer_rpc_reports_service_store_decode_errors() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let service = Arc::new(IndexerService::open_store(Arc::clone(&store)).expect("store indexer"));
    let block = block(5, Vec::new());
    service.index_block(&block).expect("index block");
    corrupt_block_by_height_record(&store, 5);
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(service),
    );

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let err = (find_handler(&handlers, "getindexerstatus").callback())(&server, &[])
        .expect_err("corrupt service-store record should fail RPC");

    assert!(
        err.to_string().contains("NeoIndexer service read failed"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn block_notification_selector_errors_name_calling_method() {
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(Arc::new(IndexerService::new())),
    );

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();

    let err = (find_handler(&handlers, "getblocknotifications").callback())(
        &server,
        &[Value::Bool(true)],
    )
    .expect_err("invalid block selector");

    assert!(
        err.to_string()
            .contains("getblocknotifications expects hash string or height integer"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_methods_require_registered_service() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();

    let err = (find_handler(&handlers, "getindexerstatus").callback())(&server, &[])
        .expect_err("service should be required");
    assert!(
        err.to_string().contains("NeoIndexer service not available"),
        "{err}"
    );
}

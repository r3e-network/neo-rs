use super::*;
use crate::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_primitives::UInt256;
use neo_storage::persistence::providers::{MemoryStore, RuntimeStore};
use neo_storage::persistence::{Store, StoreSnapshot, WriteStore};
use serde_json::json;
use std::sync::Arc;

const PREFIX_BLOCK: u8 = 0x40;
const PREFIX_TX: u8 = 0x41;

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

fn server_with_logs(store: Arc<MemoryStore>) -> RpcServer {
    let service_store = Arc::new(RuntimeStore::Memory(store.as_ref().clone()));
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_application_logs(Arc::new(
            ApplicationLogsService::new(ApplicationLogsSettings::default(), service_store),
        )),
    );
    RpcServer::new(system, RpcServerConfig::default())
}

fn persist_log<S>(store: &Arc<S>, prefix: u8, hash: &UInt256, value: serde_json::Value)
where
    S: Store,
{
    let mut snapshot_arc = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot_arc).expect("unique snapshot");
    let mut key = Vec::with_capacity(1 + 32);
    key.push(prefix);
    key.extend_from_slice(&hash.to_bytes());
    snapshot
        .put(key, serde_json::to_vec(&value).expect("serialize log"))
        .expect("put log");
    snapshot.try_commit().expect("commit log");
}

fn sample_log(hash: &UInt256) -> serde_json::Value {
    json!({
        "txid": hash.to_string(),
        "executions": [
            {
                "trigger": "Application",
                "vmstate": "HALT",
                "gasconsumed": "1",
                "exception": null,
                "stack": [],
                "notifications": []
            },
            {
                "trigger": "OnPersist",
                "vmstate": "HALT",
                "gasconsumed": "0",
                "exception": null,
                "stack": [],
                "notifications": []
            }
        ]
    })
}

#[test]
fn get_application_log_returns_transaction_log() {
    let store = Arc::new(MemoryStore::new());
    let tx_hash = UInt256::from([0x11; 32]);
    let expected = sample_log(&tx_hash);
    persist_log(&store, PREFIX_TX, &tx_hash, expected.clone());

    let server = server_with_logs(store);
    let handlers = RpcServerApplicationLogs::register_handlers();
    let handler = find_handler(&handlers, "getapplicationlog");

    let params = [serde_json::Value::String(tx_hash.to_string())];
    let actual = (handler.callback())(&server, &params).expect("application log");

    assert_eq!(actual, expected);
}

#[test]
fn get_application_log_filters_known_trigger_case_insensitively() {
    let store = Arc::new(MemoryStore::new());
    let block_hash = UInt256::from([0x22; 32]);
    persist_log(&store, PREFIX_BLOCK, &block_hash, sample_log(&block_hash));

    let server = server_with_logs(store);
    let handlers = RpcServerApplicationLogs::register_handlers();
    let handler = find_handler(&handlers, "getapplicationlog");

    let params = [
        serde_json::Value::String(block_hash.to_string()),
        serde_json::Value::String("application".to_string()),
    ];
    let actual = (handler.callback())(&server, &params).expect("filtered application log");
    let executions = actual
        .get("executions")
        .and_then(serde_json::Value::as_array)
        .expect("executions");

    assert_eq!(executions.len(), 1);
    assert_eq!(
        executions[0]
            .get("trigger")
            .and_then(serde_json::Value::as_str),
        Some("Application")
    );
}

#[test]
fn get_application_log_rejects_unknown_hash() {
    let store = Arc::new(MemoryStore::new());
    let server = server_with_logs(store);
    let handlers = RpcServerApplicationLogs::register_handlers();
    let handler = find_handler(&handlers, "getapplicationlog");

    let params = [serde_json::Value::String(
        UInt256::from([0x33; 32]).to_string(),
    )];
    let err = (handler.callback())(&server, &params).expect_err("unknown hash");
    let rpc_error: RpcError = err.into();

    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(rpc_error.data(), Some("Unknown transaction/blockhash"));
}

#[test]
fn get_application_log_requires_service() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerApplicationLogs::register_handlers();
    let handler = find_handler(&handlers, "getapplicationlog");

    let params = [serde_json::Value::String(
        UInt256::from([0x44; 32]).to_string(),
    )];
    let err = (handler.callback())(&server, &params).expect_err("missing service");
    let rpc_error: RpcError = err.into();

    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
}

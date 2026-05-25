#![cfg(feature = "jsonrpsee-server")]

use neo_core::neo_system::NeoSystem;
use neo_core::protocol_settings::ProtocolSettings;
use neo_rpc::server::{
    build_jsonrpsee_module, build_jsonrpsee_module_with_disabled, RpcException, RpcHandler,
    RpcMethodDescriptor, RpcServer, RpcServerBlockchain, RpcServerConfig, RpcServerNode,
    RpcServerUtilities, ServerRpcError, JSONRPSEE_READ_ONLY_METHODS,
};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

fn build_server_with_handlers() -> Arc<RwLock<RpcServer>> {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    Arc::new(RwLock::new(server))
}

fn find_handler(method: &str) -> RpcHandler {
    RpcServerBlockchain::register_handlers()
        .into_iter()
        .chain(RpcServerNode::register_handlers())
        .chain(RpcServerUtilities::register_handlers())
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(method))
        .expect("registered handler")
}

fn direct_handler_result(server: &Arc<RwLock<RpcServer>>, method: &str) -> Value {
    let callback = find_handler(method).callback();
    let server = server.read();
    callback(&server, &[]).expect("handler result")
}

async fn raw_response(module: &jsonrpsee::RpcModule<impl Clone>, request: Value) -> Value {
    let (response, _) = module
        .raw_json_request(&request.to_string(), 1)
        .await
        .expect("raw json response");
    serde_json::from_str(&response).expect("json response")
}

fn assert_neo_error(response: &Value, error: ServerRpcError) {
    assert_eq!(response["error"]["code"], error.code());
    assert_eq!(response["error"]["message"], error.error_message());
    if let Some(data) = error.data() {
        assert_eq!(response["error"]["data"], data);
    } else {
        assert!(response["error"].get("data").is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_initial_read_only_methods() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let methods = module.method_names().collect::<HashSet<_>>();

    for method in JSONRPSEE_READ_ONLY_METHODS {
        assert!(methods.contains(method), "missing {method}");
    }
    assert_eq!(methods.len(), JSONRPSEE_READ_ONLY_METHODS.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn initial_read_only_methods_match_registered_handlers() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");

    for (id, method) in JSONRPSEE_READ_ONLY_METHODS.iter().enumerate() {
        let response = raw_response(
            &module,
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": [],
                "id": id
            }),
        )
        .await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"], direct_handler_result(&server, method));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn getversion_accepts_omitted_params() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "getversion",
            "id": 8
        }),
    )
    .await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 8);
    assert_eq!(
        response["result"],
        direct_handler_result(&server, "getversion")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_non_array_params_with_neo_error() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": {"bad": true},
            "id": 9
        }),
    )
    .await;

    assert_neo_error(&response, ServerRpcError::invalid_request());
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_method_uses_neo_access_denied() {
    let server = build_server_with_handlers();
    let disabled = Arc::new(HashSet::from(["getblockcount".to_string()]));
    let module =
        build_jsonrpsee_module_with_disabled(Arc::downgrade(&server), disabled).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 10
        }),
    )
    .await;

    assert_neo_error(&response, ServerRpcError::access_denied());
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_error_preserves_neo_message_and_data() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblockcount"),
        Arc::new(|_, _| {
            Err(RpcException::from(
                ServerRpcError::invalid_params().with_data("height"),
            ))
        }),
    )]);
    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 11
        }),
    )
    .await;

    assert_neo_error(
        &response,
        ServerRpcError::invalid_params().with_data("height"),
    );
}

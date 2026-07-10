//! Integration tests for the jsonrpsee RPC server adapter.
#![cfg(feature = "server")]

use neo_config::ProtocolSettings;
use neo_rpc::server::{
    RpcException, RpcHandler, RpcMethodDescriptor, RpcServer, RpcServerBlockchain, RpcServerConfig,
    RpcServerIndexer, RpcServerNode, RpcServerUtilities, ServerRpcError, build_jsonrpsee_module,
    build_jsonrpsee_module_with_disabled,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_system::Node;
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

fn node_to_context(node: &Node) -> neo_rpc::server::NodeContext {
    neo_rpc::server::NodeContext::from_parts(
        node.settings(),
        Arc::new(RuntimeStore::Memory(node.storage().as_ref().clone())),
        node.blockchain(),
        node.network(),
        node.mempool(),
        node.header_cache(),
        neo_rpc::server::RpcServices::default(),
        node.native_contract_provider(),
    )
}

const JSONRPSEE_SMOKE_METHODS: &[&str] = &[
    "getbestblockhash",
    "getblockcount",
    "getblockheadercount",
    "getnativecontracts",
    "getnextblockvalidators",
    "getcandidates",
    "getconnectioncount",
    "getrawmempool",
    "getversion",
    "getindexerstatus",
    "listplugins",
    "listservices",
];

fn build_server_with_handlers() -> Arc<RwLock<RpcServer>> {
    let node =
        Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("system to start");
    let system: Arc<neo_rpc::server::NodeContext> = Arc::new(node_to_context(&node));
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    server.register_handlers(RpcServerIndexer::register_handlers());
    Arc::new(RwLock::new(server))
}

fn build_server_with_config(config: RpcServerConfig) -> RpcServer {
    let node =
        Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("system to start");
    let system: Arc<neo_rpc::server::NodeContext> = Arc::new(node_to_context(&node));
    RpcServer::new(system, config)
}

fn unused_local_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind free local port");
    listener.local_addr().expect("local address").port()
}

fn find_handler(method: &str) -> RpcHandler {
    RpcServerBlockchain::register_handlers()
        .into_iter()
        .chain(RpcServerNode::register_handlers())
        .chain(RpcServerUtilities::register_handlers())
        .chain(RpcServerIndexer::register_handlers())
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(method))
        .expect("registered handler")
}

fn direct_handler_result(
    server: &Arc<RwLock<RpcServer>>,
    method: &str,
) -> Result<Value, RpcException> {
    let callback = find_handler(method).callback();
    let server = server.read();
    callback(&server, &[])
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

fn assert_neo_exception(response: &Value, error: &RpcException) {
    assert_eq!(response["error"]["code"], error.code());
    assert_eq!(response["error"]["message"], error.to_string());
    if let Some(data) = error.data() {
        assert_eq!(response["error"]["data"], data);
    } else {
        assert!(response["error"].get("data").is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_includes_existing_smoke_methods() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let methods = module.method_names().collect::<HashSet<_>>();

    for method in JSONRPSEE_SMOKE_METHODS {
        assert!(methods.contains(method), "missing {method}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_public_methods_from_server_registry() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let methods = module
        .method_names()
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let expected = RpcServerBlockchain::register_handlers()
        .into_iter()
        .chain(RpcServerNode::register_handlers())
        .chain(RpcServerUtilities::register_handlers())
        .chain(RpcServerIndexer::register_handlers())
        .filter(|handler| !handler.descriptor().requires_auth())
        .map(|handler| handler.descriptor().name.clone())
        .collect::<HashSet<_>>();

    assert_eq!(methods, expected);
    assert!(methods.contains("getblockhash"));
    assert!(methods.contains("getindexerstatus"));
    assert!(methods.contains("getblockindexes"));
    assert!(methods.contains("getblocktransactions"));
    assert!(methods.contains("getcontracttransactions"));
    assert!(methods.contains("getaddressnotifications"));
    assert!(methods.contains("validateaddress"));
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_dynamic_public_methods_without_descriptor_api_breaks() {
    let node =
        Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("system to start");
    let system: Arc<neo_rpc::server::NodeContext> = Arc::new(node_to_context(&node));
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    let dynamic_method = ["custom", "method"].join("");
    let protected_method = ["custom", "protected"].join("");

    server.register_handlers(vec![
        RpcHandler::new(RpcMethodDescriptor::new(dynamic_method.clone()), |_, _| {
            Err(RpcException::from(
                ServerRpcError::invalid_params().with_data("dynamic"),
            ))
        }),
        RpcHandler::new(
            RpcMethodDescriptor {
                name: protected_method.clone(),
                requires_auth: true,
            },
            |_, _| Ok(json!(true)),
        ),
    ]);

    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let methods = module.method_names().collect::<HashSet<_>>();

    assert!(methods.contains(dynamic_method.as_str()));
    assert!(!methods.contains(protected_method.as_str()));

    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": dynamic_method,
            "params": [],
            "id": 12
        }),
    )
    .await;

    assert_neo_error(
        &response,
        ServerRpcError::invalid_params().with_data("dynamic"),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_protected_methods_when_rpc_auth_is_configured() {
    let mut config = RpcServerConfig {
        rpc_user: "neo".to_string(),
        rpc_pass: "secret".to_string(),
        ..RpcServerConfig::default()
    };
    config.port = unused_local_port();
    let protected_method = "customprotected".to_string();
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor {
            name: protected_method.clone(),
            requires_auth: true,
        },
        |_, _| Ok(json!(true)),
    )]);

    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let methods = module.method_names().collect::<HashSet<_>>();

    assert!(methods.contains(protected_method.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_method_without_transport_auth_returns_access_denied() {
    let config = RpcServerConfig {
        rpc_user: "neo".to_string(),
        rpc_pass: "secret".to_string(),
        ..RpcServerConfig::default()
    };
    let protected_method = "customprotected".to_string();
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor {
            name: protected_method.clone(),
            requires_auth: true,
        },
        |_, _| Ok(json!(true)),
    )]);

    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": protected_method,
            "params": [],
            "id": 14
        }),
    )
    .await;

    assert_neo_error(&response, ServerRpcError::access_denied());
}

#[tokio::test(flavor = "multi_thread")]
async fn http_transport_enforces_basic_auth_when_configured() {
    let port = unused_local_port();
    let config = RpcServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        rpc_user: "neo".to_string(),
        rpc_pass: "secret".to_string(),
        ..RpcServerConfig::default()
    };
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblockcount"),
        |_, _| Ok(json!(42)),
    )]);
    let server = Arc::new(RwLock::new(server));
    server
        .write()
        .start_rpc_server(Arc::downgrade(&server), None);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}");
    let request = json!({
        "jsonrpc": "2.0",
        "method": "getblockcount",
        "params": [],
        "id": 15
    });

    let unauthorized = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("unauthorized request");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let authorized: Value = client
        .post(&url)
        .basic_auth("neo", Some("secret"))
        .json(&request)
        .send()
        .await
        .expect("authorized request")
        .json()
        .await
        .expect("json response");
    assert_eq!(authorized["result"], json!(42));

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn http_transport_emits_cors_headers_for_allowed_origin() {
    let port = unused_local_port();
    let origin = "https://dapp.example";
    let config = RpcServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        enable_cors: true,
        allow_origins: vec![origin.to_string()],
        ..RpcServerConfig::default()
    };
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblockcount"),
        |_, _| Ok(json!(42)),
    )]);
    let server = Arc::new(RwLock::new(server));
    server
        .write()
        .start_rpc_server(Arc::downgrade(&server), None);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}");
    let response = client
        .post(&url)
        .header("origin", origin)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 16
        }))
        .send()
        .await
        .expect("cors request");

    assert_eq!(response.headers()["access-control-allow-origin"], origin);
    let body: Value = response.json().await.expect("json response");
    assert_eq!(body["result"], json!(42));

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn http_transport_answers_cors_preflight_without_basic_auth() {
    let port = unused_local_port();
    let origin = "https://wallet.example";
    let config = RpcServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        rpc_user: "neo".to_string(),
        rpc_pass: "secret".to_string(),
        enable_cors: true,
        allow_origins: vec![origin.to_string()],
        ..RpcServerConfig::default()
    };
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblockcount"),
        |_, _| Ok(json!(42)),
    )]);
    let server = Arc::new(RwLock::new(server));
    server
        .write()
        .start_rpc_server(Arc::downgrade(&server), None);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}");
    let preflight = client
        .request(reqwest::Method::OPTIONS, &url)
        .header("origin", origin)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "content-type, authorization",
        )
        .send()
        .await
        .expect("cors preflight");

    assert_eq!(preflight.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(preflight.headers()["access-control-allow-origin"], origin);
    assert_eq!(
        preflight.headers()["access-control-allow-headers"],
        "content-type, authorization"
    );

    let unauthorized = client
        .post(&url)
        .header("origin", origin)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 17
        }))
        .send()
        .await
        .expect("unauthorized cors request");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthorized.headers()["access-control-allow-origin"],
        origin
    );

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn initial_read_only_methods_match_registered_handlers() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");

    for (id, method) in JSONRPSEE_SMOKE_METHODS.iter().enumerate() {
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
        match direct_handler_result(&server, method) {
            Ok(result) => assert_eq!(response["result"], result),
            Err(error) => assert_neo_exception(&response, &error),
        }
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
        direct_handler_result(&server, "getversion").expect("getversion result")
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
async fn dispatch_enforces_configured_process_rate_limit() {
    let config = RpcServerConfig {
        max_requests_per_second: 1,
        rate_limit_burst: 1,
        ..RpcServerConfig::default()
    };
    let mut server = build_server_with_config(config);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblock"),
        |_, _| Ok(json!("ok")),
    )]);
    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");

    let request = |id| {
        json!({
            "jsonrpc": "2.0",
            "method": "getblock",
            "params": [],
            "id": id
        })
    };

    let first = raw_response(&module, request(1)).await;
    assert_eq!(first["result"], json!("ok"));

    let second = raw_response(&module, request(2)).await;
    assert_neo_error(&second, ServerRpcError::too_many_requests());
}

#[tokio::test(flavor = "multi_thread")]
async fn unregistered_method_is_rejected_by_jsonrpsee_before_neo_dispatch() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server)).expect("module");
    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "missingmethod",
            "params": [],
            "id": 13
        }),
    )
    .await;

    assert_eq!(
        response["error"]["code"],
        ServerRpcError::method_not_found().code()
    );
    assert_eq!(response["error"]["message"], "Method not found");
    assert!(response["error"].get("data").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_error_preserves_neo_message_and_data() {
    let node =
        Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("system to start");
    let system: Arc<neo_rpc::server::NodeContext> = Arc::new(node_to_context(&node));
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("getblockcount"),
        |_, _| {
            Err(RpcException::from(
                ServerRpcError::invalid_params().with_data("height"),
            ))
        },
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

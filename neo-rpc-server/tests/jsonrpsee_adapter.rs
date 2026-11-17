#![cfg(feature = "jsonrpsee-server")]

use jsonrpsee::RpcModule;
use jsonrpsee::core::middleware::RpcServiceBuilder;
use jsonrpsee::server::{Server, ServerHandle};
use neo_protocol::ProtocolSettings;
use neo_rpc_server::{
    JsonRpseeAdapterError, JsonRpseeBatchLimits, JsonRpseeContext, RpcException, RpcHandler,
    RpcMethodDescriptor, RpcServer, RpcServerBlockchain, RpcServerConfig, RpcServerNode,
    RpcServerUtilities, ServerRpcError, build_jsonrpsee_module,
    build_jsonrpsee_module_with_disabled, build_jsonrpsee_module_with_external_auth,
    jsonrpsee_server_config,
};
use neo_system::NeoSystem;
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
    "listplugins",
];

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
    serde_json::from_str(response.get()).expect("json response")
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
async fn module_includes_existing_smoke_methods() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
    let methods = module.method_names().collect::<HashSet<_>>();

    for method in JSONRPSEE_SMOKE_METHODS {
        assert!(methods.contains(method), "missing {method}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_public_methods_from_server_registry() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
    let methods = module
        .method_names()
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let expected = RpcServerBlockchain::register_handlers()
        .into_iter()
        .chain(RpcServerNode::register_handlers())
        .chain(RpcServerUtilities::register_handlers())
        .filter(|handler| !handler.descriptor().requires_auth())
        .map(|handler| handler.descriptor().name.clone())
        .collect::<HashSet<_>>();

    assert_eq!(methods, expected);
    assert!(methods.contains("getblockhash"));
    assert!(methods.contains("validateaddress"));
}

#[tokio::test(flavor = "multi_thread")]
async fn module_registers_dynamic_public_methods_without_descriptor_api_breaks() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    let dynamic_method = ["custom", "method"].join("");
    let protected_method = ["custom", "protected"].join("");

    server.register_handlers(vec![
        RpcHandler::new(
            RpcMethodDescriptor::new(dynamic_method.clone()),
            Arc::new(|_, _| {
                Err(RpcException::from(
                    ServerRpcError::invalid_params().with_data("dynamic"),
                ))
            }),
        ),
        RpcHandler::new(
            RpcMethodDescriptor {
                name: protected_method.clone(),
                requires_auth: true,
            },
            Arc::new(|_, _| Ok(json!(true))),
        ),
    ]);

    let server = Arc::new(RwLock::new(server));
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
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
async fn initial_read_only_methods_match_registered_handlers() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");

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
        assert_eq!(response["result"], direct_handler_result(&server, method));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn getversion_accepts_omitted_params() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
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
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
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
    let module = build_jsonrpsee_module_with_disabled(
        Arc::downgrade(&server),
        &RpcServerConfig::default(),
        disabled,
    )
    .expect("module");
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
async fn unregistered_method_is_rejected_by_jsonrpsee_before_neo_dispatch() {
    let server = build_server_with_handlers();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
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
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");
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

#[tokio::test(flavor = "multi_thread")]
async fn configured_disabled_methods_are_denied_case_insensitively() {
    let server = build_server_with_handlers();
    let config = RpcServerConfig {
        disabled_methods: vec!["GetBlockCount".to_string()],
        ..Default::default()
    };
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");

    let response = raw_response(
        &module,
        json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 12
        }),
    )
    .await;
    assert_neo_error(&response, ServerRpcError::access_denied());

    // Other methods keep being served.
    assert!(module.method_names().any(|name| name == "getblockhash"));
}

#[tokio::test(flavor = "multi_thread")]
async fn credentialed_config_requires_external_authentication() {
    let server = build_server_with_handlers();
    let config = RpcServerConfig {
        rpc_user: "neo".to_string(),
        rpc_pass: "secret".to_string(),
        ..Default::default()
    };

    let Err(error) = build_jsonrpsee_module(Arc::downgrade(&server), &config) else {
        panic!("adapter must refuse to build without an auth gate");
    };
    assert!(matches!(
        error,
        JsonRpseeAdapterError::AuthenticationRequired
    ));

    // An explicit acknowledgement that authentication terminates in front of
    // the jsonrpsee server is accepted.
    let module = build_jsonrpsee_module_with_external_auth(
        Arc::downgrade(&server),
        &config,
        Arc::new(HashSet::new()),
    )
    .expect("module with external auth");
    assert!(module.method_names().any(|name| name == "getversion"));
}

/// A server whose only method, `probe`, counts the calls that reach it.
fn build_probe_server() -> (Arc<RwLock<RpcServer>>, Arc<AtomicUsize>) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("probe"),
        Arc::new(move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(json!(true))
        }),
    )]);
    (Arc::new(RwLock::new(server)), calls)
}

/// A call to `probe` with `params` given as JSON text, kept byte for byte.
fn probe_request(params: &str, id: u32) -> String {
    format!(r#"{{"jsonrpc":"2.0","method":"probe","params":{params},"id":{id}}}"#)
}

/// `depth` arrays, one inside the other.
fn nested_arrays(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

/// An array holding one string, `len` bytes long as JSON text.
fn params_of_len(len: usize) -> String {
    format!(r#"["{}"]"#, "a".repeat(len - 4))
}

async fn raw_text_response(module: &RpcModule<JsonRpseeContext>, request: &str) -> Value {
    let (response, _) = module
        .raw_json_request(request, 1)
        .await
        .expect("raw json response");
    serde_json::from_str(response.get()).expect("json response")
}

/// Serves `module` the way the adapter documents: with the limits of `config`
/// and, when `batch_limits` is set, the batch middleware.
async fn serve(
    module: RpcModule<JsonRpseeContext>,
    config: &RpcServerConfig,
    batch_limits: bool,
) -> (SocketAddr, ServerHandle) {
    let builder = Server::builder().set_config(jsonrpsee_server_config(config).build());
    if batch_limits {
        let server = builder
            .set_rpc_middleware(RpcServiceBuilder::new().layer_fn(JsonRpseeBatchLimits::new))
            .build("127.0.0.1:0")
            .await
            .expect("server to bind");
        (
            server.local_addr().expect("bound address"),
            server.start(module),
        )
    } else {
        let server = builder.build("127.0.0.1:0").await.expect("server to bind");
        (
            server.local_addr().expect("bound address"),
            server.start(module),
        )
    }
}

/// Sends one HTTP request and returns the status and body of the response,
/// or `None` when the server hung up without a complete one.
async fn exchange(address: SocketAddr, request: &[u8]) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream.write_all(request).await.expect("send request");

    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(response) = complete_response(&received) {
            return Some(response);
        }
        let read = tokio::time::timeout(Duration::from_secs(30), stream.read(&mut chunk))
            .await
            .expect("the server to answer or hang up");
        match read {
            Ok(read) if read > 0 => received.extend_from_slice(&chunk[..read]),
            _ => return None,
        }
    }
}

fn complete_response(received: &[u8]) -> Option<(u16, String)> {
    let text = std::str::from_utf8(received).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head.split(' ').nth(1)?.parse().ok()?;
    let len = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    })?;
    (body.len() >= len).then(|| (status, body[..len].to_string()))
}

fn post_head(address: SocketAddr, framing: &str) -> String {
    format!(
        "POST / HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
         Connection: close\r\n{framing}\r\n\r\n"
    )
}

/// Posts `body` and returns the status and the JSON the server answered with.
async fn post(address: SocketAddr, body: &str) -> (u16, Value) {
    let head = post_head(address, &format!("Content-Length: {}", body.len()));
    let (status, response) = exchange(address, format!("{head}{body}").as_bytes())
        .await
        .expect("response");
    (
        status,
        serde_json::from_str(&response).expect("json response"),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn params_at_the_depth_limit_are_served_and_one_level_deeper_refused() {
    let (server, calls) = build_probe_server();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");

    // The limit is 32 levels over the whole request, as on the warp front
    // end. The request object is the first, which leaves 31 for `params`.
    let response = raw_text_response(&module, &probe_request(&nested_arrays(31), 1)).await;
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let response = raw_text_response(&module, &probe_request(&nested_arrays(32), 2)).await;
    assert_neo_error(&response, ServerRpcError::bad_request());
    assert_eq!(response["id"], 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Objects count like arrays, and a scalar is a level of its own.
    let objects = |depth: usize| format!("[{}0{}]", r#"{"k":"#.repeat(depth), "}".repeat(depth));
    let response = raw_text_response(&module, &probe_request(&objects(29), 3)).await;
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let response = raw_text_response(&module, &probe_request(&objects(30), 4)).await;
    assert_neo_error(&response, ServerRpcError::bad_request());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn hundred_thousand_levels_are_refused_before_parsing_on_a_small_stack() {
    const SMALL_STACK: usize = 128 * 1024;

    let (server, calls) = build_probe_server();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");

    // The call runs where the adapter's methods run, on a blocking thread of
    // the runtime driving it. Every thread of this one has a small stack.
    let small = std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_stack_size(SMALL_STACK)
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async {
                let served = raw_text_response(&module, &probe_request("[[1]]", 1)).await;
                let deep = probe_request(&nested_arrays(100_000), 2);
                (served, raw_text_response(&module, &deep).await)
            })
        })
        .expect("thread to start");
    let (served, refused) = tokio::task::spawn_blocking(move || small.join())
        .await
        .expect("join task")
        .expect("requests to finish");

    assert_eq!(served["result"], true);
    // The adapter's own refusal: a parser that got this far would have
    // answered with its error, as invalid params.
    assert_neo_error(&refused, ServerRpcError::bad_request());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn params_longer_than_the_body_limit_are_refused_by_the_module() {
    const LIMIT: usize = 256;

    let (server, calls) = build_probe_server();
    let config = RpcServerConfig {
        max_request_body_size: LIMIT,
        ..Default::default()
    };
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");

    let response = raw_text_response(&module, &probe_request(&params_of_len(LIMIT), 1)).await;
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let response = raw_text_response(&module, &probe_request(&params_of_len(LIMIT + 1), 2)).await;
    assert_neo_error(&response, ServerRpcError::bad_request());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn server_serves_a_body_at_the_size_limit_and_refuses_a_larger_one() {
    const LIMIT: usize = 256;

    let (server, calls) = build_probe_server();
    let config = RpcServerConfig {
        max_request_body_size: LIMIT,
        ..Default::default()
    };
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");
    let (address, _handle) = serve(module, &config, true).await;

    let envelope = probe_request("", 1).len();
    let body = probe_request(&params_of_len(LIMIT - envelope), 1);
    assert_eq!(body.len(), LIMIT);
    let (status, response) = post(address, &body).await;
    assert_eq!(status, 200);
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // One byte more is refused on its declared length alone: only the head
    // is sent.
    let head = post_head(address, &format!("Content-Length: {}", LIMIT + 1));
    let (status, _) = exchange(address, head.as_bytes()).await.expect("response");
    assert_eq!(status, 413);

    // The same body without a declared length is cut off while it is read.
    let body = probe_request(&params_of_len(LIMIT + 1 - envelope), 2);
    let head = post_head(address, "Transfer-Encoding: chunked");
    let chunked = format!("{head}{:x}\r\n{body}\r\n0\r\n\r\n", body.len());
    let outcome = exchange(address, chunked.as_bytes()).await;
    assert!(outcome.is_none_or(|(status, _)| status != 200));

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_with_an_over_deep_entry_is_refused_whole() {
    let (server, calls) = build_probe_server();
    let config = RpcServerConfig::default();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");
    let (address, _handle) = serve(module, &config, true).await;

    // In a batch the array around the requests is a level too, which leaves
    // 30 for `params`.
    let batch = |depth: usize| {
        format!(
            "[{},{}]",
            probe_request("[]", 1),
            probe_request(&nested_arrays(depth), 2)
        )
    };

    let (status, response) = post(address, &batch(30)).await;
    assert_eq!(status, 200);
    let entries = response.as_array().expect("batch response");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry["result"] == true));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // One entry too deep refuses them all, the one that was fine included.
    let (status, response) = post(address, &batch(31)).await;
    assert_eq!(status, 200);
    assert_neo_error(&response, ServerRpcError::bad_request());
    assert_eq!(response["id"], Value::Null);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let (_, response) = post(address, &batch(100_000)).await;
    assert_neo_error(&response, ServerRpcError::bad_request());
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // Alone, the same request has the level the batch array took.
    let (_, response) = post(address, &probe_request(&nested_arrays(31), 3)).await;
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn module_refuses_an_over_deep_batch_entry_without_the_middleware() {
    let (server, calls) = build_probe_server();
    let config = RpcServerConfig::default();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");
    let (address, _handle) = serve(module, &config, false).await;

    let batch = format!(
        "[{},{},{}]",
        probe_request("[]", 1),
        probe_request(&nested_arrays(32), 2),
        probe_request(&nested_arrays(100_000), 3)
    );
    let (status, response) = post(address, &batch).await;
    assert_eq!(status, 200);

    let entries = response.as_array().expect("batch response");
    assert_eq!(entries.len(), 3);
    for entry in entries {
        if entry["id"] == 1 {
            assert_eq!(entry["result"], true);
        } else {
            assert_neo_error(entry, ServerRpcError::bad_request());
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn server_refuses_a_batch_over_the_configured_size() {
    let (server, calls) = build_probe_server();
    let config = RpcServerConfig {
        max_batch_size: 2,
        ..Default::default()
    };
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &config).expect("module");
    let (address, _handle) = serve(module, &config, true).await;

    let batch = |len: u32| {
        let entries = (1..=len)
            .map(|id| probe_request("[]", id))
            .collect::<Vec<_>>();
        format!("[{}]", entries.join(","))
    };

    let (_, response) = post(address, &batch(2)).await;
    assert_eq!(response.as_array().expect("batch response").len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let (_, response) = post(address, &batch(3)).await;
    assert!(response["error"].is_object(), "{response}");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn method_that_turns_protected_after_the_build_is_refused() {
    let (server, calls) = build_probe_server();
    let module = build_jsonrpsee_module(Arc::downgrade(&server), &RpcServerConfig::default())
        .expect("module");

    let response = raw_text_response(&module, &probe_request("[]", 1)).await;
    assert_eq!(response["result"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // The registry is live: a handler registered later under the same name
    // replaces the one the module was built from.
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&protected_calls);
    server.write().register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor {
            name: "probe".to_string(),
            requires_auth: true,
        },
        Arc::new(move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(json!("protected"))
        }),
    )]);

    let response = raw_text_response(&module, &probe_request("[]", 2)).await;
    assert_neo_error(&response, ServerRpcError::access_denied());
    assert_eq!(protected_calls.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

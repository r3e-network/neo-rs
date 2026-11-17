use super::cors::verify_basic_auth;
use super::handlers::process_body;
use super::*;
use crate::middleware::{RateLimitCheckResult, RateLimitTier};
use crate::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::RpcHandler;
use crate::rpc_server_blockchain::RpcServerBlockchain;
use crate::rpc_server_node::RpcServerNode;
use crate::rpc_server_settings::RpcServerConfig;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::BinaryWriter;
use neo_native_contracts::native::LedgerContract;
use neo_primitives::WitnessScope;
use neo_protocol::ProtocolSettings;
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::Signer;
use neo_protocol::payloads::transaction::Transaction;
use neo_protocol::witness::Witness;
use neo_storage::{StorageItem, StorageKey};
use neo_system::NeoSystem;
use neo_vm::OpCode;
use neo_vm::VmState as VMState;
use neo_wallets::KeyPair;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use warp::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, VARY};

fn build_test_routes(
    settings: RpcServerConfig,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    let handle: Weak<RwLock<RpcServer>> = Weak::new();
    let disabled: Arc<HashSet<String>> = Arc::new(HashSet::new());
    let auth: Arc<Option<BasicAuth>> = Arc::new(None);
    build_rpc_routes(handle, disabled, auth, settings)
}

fn build_filters_with_handlers() -> (Arc<RwLock<RpcServer>>, RpcFilters) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());

    let server = Arc::new(RwLock::new(server));
    let filters = RpcFilters {
        server: Arc::downgrade(&server),
        disabled: Arc::new(HashSet::new()),
        auth: Arc::new(None),
        rate_limiter: None,
        cors: None,
        max_batch_size: 1024,
    };
    (server, filters)
}

#[test]
fn per_method_rate_limiting_blocks_expensive_methods() {
    use std::net::IpAddr;

    let config = RateLimitConfig {
        max_rps: 100,
        burst: 100,
    };
    let limiter = Arc::new(GovernorRateLimiter::new(config.clone()));
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(
        RateLimitTier::from_method("invokefunction"),
        RateLimitTier::Expensive
    );
    assert_eq!(
        RateLimitTier::from_method("sendrawtransaction"),
        RateLimitTier::Write
    );
    assert_eq!(
        RateLimitTier::from_method("getblockcount"),
        RateLimitTier::Cheap
    );

    let expensive_config = limiter.tier_config(RateLimitTier::Expensive).unwrap();
    assert!(expensive_config.max_rps < config.max_rps);

    let result = limiter.check_for_method(ip, "invokefunction");
    assert!(result.is_allowed());
}

#[test]
fn rate_limit_check_result_is_handled_properly() {
    let allowed = RateLimitCheckResult::Allowed;
    let blocked = RateLimitCheckResult::Blocked;
    let disabled = RateLimitCheckResult::Disabled;

    assert!(allowed.is_allowed());
    assert!(!allowed.is_blocked());

    assert!(!blocked.is_allowed());
    assert!(blocked.is_blocked());

    assert!(disabled.is_allowed());
    assert!(!disabled.is_blocked());
}

fn build_filters_with_panic_handler() -> (Arc<RwLock<RpcServer>>, RpcFilters) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("panic"),
        Arc::new(|_, _| panic!("boom")),
    )]);

    let server = Arc::new(RwLock::new(server));
    let filters = RpcFilters {
        server: Arc::downgrade(&server),
        disabled: Arc::new(HashSet::new()),
        auth: Arc::new(None),
        rate_limiter: None,
        cors: None,
        max_batch_size: 1024,
    };
    (server, filters)
}

fn build_filters_with_counting_handler(
    invocations: Arc<AtomicUsize>,
) -> (Arc<RwLock<RpcServer>>, RpcFilters) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(vec![RpcHandler::new(
        RpcMethodDescriptor::new("ping"),
        Arc::new(move |_, _| {
            invocations.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }),
    )]);

    let server = Arc::new(RwLock::new(server));
    let filters = RpcFilters {
        server: Arc::downgrade(&server),
        disabled: Arc::new(HashSet::new()),
        auth: Arc::new(None),
        rate_limiter: None,
        cors: None,
        max_batch_size: 1024,
    };
    (server, filters)
}

fn build_filters_with_auth(
    auth: Arc<Option<BasicAuth>>,
    include_wallet: bool,
) -> (Arc<RwLock<RpcServer>>, RpcFilters) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    if include_wallet {
        server.register_handlers(crate::rpc_server_wallet::RpcServerWallet::register_handlers());
    }

    let server = Arc::new(RwLock::new(server));
    let filters = RpcFilters {
        server: Arc::downgrade(&server),
        disabled: Arc::new(HashSet::new()),
        auth,
        rate_limiter: None,
        cors: None,
        max_batch_size: 1024,
    };
    (server, filters)
}

#[test]
fn verify_basic_auth_accepts_valid_credentials() {
    let auth = BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    };
    let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:testpass"));
    assert!(verify_basic_auth(Some(&header), &auth));
}

#[test]
fn verify_basic_auth_rejects_invalid_credentials() {
    let auth = BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    };
    let wrong_user = format!("Basic {}", BASE64_STANDARD.encode("wrong:testpass"));
    let wrong_pass = format!("Basic {}", BASE64_STANDARD.encode("testuser:wrong"));
    let wrong_scheme = format!("Bearer {}", BASE64_STANDARD.encode("testuser:testpass"));

    assert!(!verify_basic_auth(Some(&wrong_user), &auth));
    assert!(!verify_basic_auth(Some(&wrong_pass), &auth));
    assert!(!verify_basic_auth(Some(&wrong_scheme), &auth));
    assert!(!verify_basic_auth(None, &auth));
}

#[tokio::test]
async fn cors_echoes_matching_origin_from_allowlist() {
    let settings = RpcServerConfig {
        enable_cors: true,
        allow_origins: vec!["https://a.example".into(), "https://b.example".into()],
        ..Default::default()
    };

    let routes = build_test_routes(settings);

    let resp = warp::test::request()
        .method("OPTIONS")
        .path("/")
        .header("origin", "https://b.example")
        .reply(&routes)
        .await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("https://b.example")
    );
    assert_eq!(
        resp.headers().get(VARY).and_then(|v| v.to_str().ok()),
        Some("origin")
    );
}

#[tokio::test]
async fn cors_omits_allow_origin_for_disallowed_origin() {
    let settings = RpcServerConfig {
        enable_cors: true,
        allow_origins: vec!["https://a.example".into()],
        ..Default::default()
    };

    let routes = build_test_routes(settings);

    let resp = warp::test::request()
        .method("OPTIONS")
        .path("/")
        .header("origin", "https://b.example")
        .reply(&routes)
        .await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    assert_eq!(
        resp.headers().get(VARY).and_then(|v| v.to_str().ok()),
        Some("origin")
    );
}

#[tokio::test]
async fn cors_wildcard_allows_any_origin() {
    let settings = RpcServerConfig {
        enable_cors: true,
        allow_origins: Vec::new(),
        ..Default::default()
    };

    let routes = build_test_routes(settings);

    let resp = warp::test::request()
        .method("OPTIONS")
        .path("/")
        .header("origin", "https://whatever.example")
        .reply(&routes)
        .await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("*")
    );
    assert!(resp.headers().get(VARY).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_rejects_malformed_json() {
    let (_server, filters) = build_filters_with_handlers();
    let (response, unauthorized) = process_body(&filters, None, b"{ invalid json", None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    let code = error.get("code").and_then(Value::as_i64).expect("code");
    assert_eq!(code, RpcError::bad_request().code() as i64);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_rejects_empty_batch() {
    let (_server, filters) = build_filters_with_handlers();
    let (response, unauthorized) = process_body(&filters, None, b"[]", None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    let code = error.get("code").and_then(Value::as_i64).expect("code");
    assert_eq!(code, RpcError::invalid_request().code() as i64);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_mixed_batch() {
    let (_server, filters) = build_filters_with_handlers();
    let body = br#"[
        {"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1},
        {"jsonrpc": "2.0", "method": "nonexistentmethod", "params": [], "id": 2},
        {"jsonrpc": "2.0", "method": "getblock", "params": ["invalid_index"], "id": 3},
        {"jsonrpc": "2.0", "method": "getversion", "id": 4}
    ]"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let batch = response.as_array().expect("batch array");
    assert_eq!(batch.len(), 4);

    let first = batch[0].as_object().expect("first response");
    assert!(first.get("error").is_none());
    assert!(first.get("result").is_some());
    assert_eq!(first.get("id").and_then(Value::as_i64), Some(1));

    let second = batch[1].as_object().expect("second response");
    let second_error = second
        .get("error")
        .and_then(Value::as_object)
        .expect("second error");
    assert_eq!(
        second_error.get("code").and_then(Value::as_i64),
        Some(RpcError::method_not_found().code() as i64)
    );
    assert_eq!(second.get("id").and_then(Value::as_i64), Some(2));

    let third = batch[2].as_object().expect("third response");
    let third_error = third
        .get("error")
        .and_then(Value::as_object)
        .expect("third error");
    assert_eq!(
        third_error.get("code").and_then(Value::as_i64),
        Some(RpcError::invalid_params().code() as i64)
    );
    assert_eq!(third.get("id").and_then(Value::as_i64), Some(3));

    let fourth = batch[3].as_object().expect("fourth response");
    assert!(fourth.get("error").is_none());
    assert!(fourth.get("result").is_some());
    assert_eq!(fourth.get("id").and_then(Value::as_i64), Some(4));
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_executes_notifications_without_response() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let (_server, filters) = build_filters_with_counting_handler(Arc::clone(&invocations));
    let body = br#"{"jsonrpc": "2.0", "method": "ping", "params": []}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);

    assert!(!unauthorized);
    assert!(response.is_none());
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_authenticates_notifications_without_json_response() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let (_server, mut filters) = build_filters_with_counting_handler(Arc::clone(&invocations));
    filters.auth = Arc::new(Some(BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    }));
    let body = br#"{"jsonrpc": "2.0", "method": "ping", "params": []}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(unauthorized);
    assert!(response.is_none());
    assert_eq!(invocations.load(Ordering::SeqCst), 0);

    let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:testpass"));
    let (response, unauthorized) = process_body(&filters, Some(&header), body, None);
    assert!(!unauthorized);
    assert!(response.is_none());
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_applies_method_rate_limit_to_notifications() {
    let invocations = Arc::new(AtomicUsize::new(0));
    let (_server, mut filters) = build_filters_with_counting_handler(Arc::clone(&invocations));
    filters.rate_limiter = Some(Arc::new(GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 1,
        burst: 1,
    })));
    let client_ip = "127.0.0.1".parse().expect("client IP");
    let body = br#"{"jsonrpc": "2.0", "method": "ping", "params": []}"#;

    for expected in 1..=2 {
        let (response, unauthorized) = process_body(&filters, None, body, Some(client_ip));
        assert!(!unauthorized);
        assert!(response.is_none());
        assert_eq!(invocations.load(Ordering::SeqCst), expected);
    }

    let (response, unauthorized) = process_body(&filters, None, body, Some(client_ip));
    assert!(!unauthorized);
    assert!(response.is_none());
    assert_eq!(invocations.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_reports_already_exists_for_sendrawtransaction() {
    let (server, filters) = build_filters_with_handlers();
    let settings = ProtocolSettings::default();
    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let tx = build_signed_transaction(&settings, &keypair, 2, 0);
    let mut store = server.read().system().context().store_snapshot_cache();
    persist_transaction_record(&mut store, &tx, 1);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendrawtransaction",
        "params": [payload],
    });
    let body = serde_json::to_vec(&request).expect("serialize body");
    let (response, unauthorized) =
        tokio::task::block_in_place(|| process_body(&filters, None, &body, None));
    assert!(!unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(RpcError::already_exists().code() as i64)
    );
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(message.contains(RpcError::already_exists().message()));
}

fn build_signed_transaction(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
    nonce: u32,
    system_fee: i64,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(1_0000_0000);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn persist_transaction_record(
    store: &mut neo_storage::persistence::StoreCache,
    tx: &Transaction,
    block_index: u32,
) {
    const PREFIX_TRANSACTION: u8 = 0x0b;
    const RECORD_KIND_TRANSACTION: u8 = 0x01;

    let mut writer = BinaryWriter::new();
    writer
        .write_u8(RECORD_KIND_TRANSACTION)
        .expect("record kind");
    writer.write_u32(block_index).expect("block index");
    writer.write_u8(VMState::NONE.to_byte()).expect("vm state");
    let tx_bytes = tx.to_bytes();
    writer.write_var_bytes(&tx_bytes).expect("tx bytes");

    let mut key_bytes = Vec::with_capacity(1 + 32);
    key_bytes.push(PREFIX_TRANSACTION);
    key_bytes.extend_from_slice(&tx.hash().to_bytes());
    let key = StorageKey::new(LedgerContract::ID, key_bytes);
    store.add(key, StorageItem::from_bytes(writer.to_bytes()));
    store.commit();
}

/// Registers a public `ping` and a protected `secret`, each counting how often
/// its handler body actually runs.
fn build_server_with_protected_counter(
    public_calls: Arc<AtomicUsize>,
    protected_calls: Arc<AtomicUsize>,
) -> Arc<RwLock<RpcServer>> {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(vec![
        RpcHandler::new(
            RpcMethodDescriptor::new("ping"),
            Arc::new(move |_, _| {
                public_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::String("pong".to_string()))
            }),
        ),
        RpcHandler::new(
            RpcMethodDescriptor::new_protected("secret"),
            Arc::new(move |_, _| {
                protected_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::String("classified".to_string()))
            }),
        ),
    ]);
    Arc::new(RwLock::new(server))
}

fn filters_for(server: &Arc<RwLock<RpcServer>>, auth: Option<BasicAuth>) -> RpcFilters {
    RpcFilters {
        server: Arc::downgrade(server),
        disabled: Arc::new(HashSet::new()),
        auth: Arc::new(auth),
        rate_limiter: None,
        cors: None,
        max_batch_size: 1024,
    }
}

fn routes_for(
    server: &Arc<RwLock<RpcServer>>,
    auth: Option<BasicAuth>,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone + use<> {
    build_rpc_routes(
        Arc::downgrade(server),
        Arc::new(HashSet::new()),
        Arc::new(auth),
        RpcServerConfig::default(),
    )
}

fn test_credentials() -> BasicAuth {
    BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    }
}

fn basic_header(user: &str, pass: &str) -> String {
    format!("Basic {}", BASE64_STANDARD.encode(format!("{user}:{pass}")))
}

fn error_code(response: &Value) -> Option<i64> {
    response
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
}

const ACCESS_DENIED: i64 = -600;

#[test]
fn access_denied_code_matches_rpc_error() {
    assert_eq!(RpcError::access_denied().code() as i64, ACCESS_DENIED);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_denies_wallet_method_without_auth_config() {
    let (_server, filters) = build_filters_with_auth(Arc::new(None), true);
    let body = br#"{"jsonrpc": "2.0", "method": "getnewaddress", "params": [], "id": 1}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(unauthorized);

    let response = response.expect("response");
    assert_eq!(response.get("id").and_then(Value::as_i64), Some(1));
    // Denied by the dispatcher, not by the wallet handler's own
    // "no opened wallet" check.
    assert_eq!(error_code(&response), Some(ACCESS_DENIED));
}

#[tokio::test(flavor = "multi_thread")]
async fn every_wallet_method_is_denied_without_auth_config() {
    let (_server, filters) = build_filters_with_auth(Arc::new(None), true);
    let methods = crate::rpc_server_wallet::RpcServerWallet::register_handlers()
        .iter()
        .map(|handler| handler.descriptor().name.clone())
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 13);
    for name in ["dumpprivkey", "sendtoaddress", "openwallet"] {
        assert!(methods.iter().any(|method| method == name), "{name}");
    }

    for method in methods {
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "method": method, "params": [], "id": 1,
        }))
        .expect("body");
        let (response, unauthorized) = process_body(&filters, None, &body, None);
        assert!(unauthorized, "{method}");
        assert_eq!(
            error_code(&response.expect("response")),
            Some(ACCESS_DENIED),
            "{method}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_method_reaches_its_handler_with_valid_credentials() {
    let auth = Arc::new(Some(test_credentials()));
    let (_server, filters) = build_filters_with_auth(auth, true);
    let body = br#"{"jsonrpc": "2.0", "method": "getnewaddress", "params": [], "id": 1}"#;

    let header = basic_header("testuser", "testpass");
    let (response, unauthorized) = process_body(&filters, Some(&header), body, None);
    assert!(!unauthorized);
    assert_eq!(
        error_code(&response.expect("response")),
        Some(RpcError::no_opened_wallet().code() as i64)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_method_succeeds_with_valid_credentials() {
    let public_calls = Arc::new(AtomicUsize::new(0));
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let server = build_server_with_protected_counter(
        Arc::clone(&public_calls),
        Arc::clone(&protected_calls),
    );
    let filters = filters_for(&server, Some(test_credentials()));
    let body = br#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 7}"#;

    let header = basic_header("testuser", "testpass");
    let (response, unauthorized) = process_body(&filters, Some(&header), body, None);
    assert!(!unauthorized);
    let response = response.expect("response");
    assert_eq!(
        response.get("result").and_then(Value::as_str),
        Some("classified")
    );
    assert_eq!(response.get("id").and_then(Value::as_i64), Some(7));
    assert_eq!(protected_calls.load(Ordering::SeqCst), 1);

    for header in [None, Some(basic_header("testuser", "wrong"))] {
        let (response, unauthorized) = process_body(&filters, header.as_deref(), body, None);
        assert!(unauthorized);
        assert_eq!(
            error_code(&response.expect("response")),
            Some(ACCESS_DENIED)
        );
    }
    assert_eq!(protected_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_method_is_never_invoked_without_auth_config() {
    let public_calls = Arc::new(AtomicUsize::new(0));
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let server = build_server_with_protected_counter(
        Arc::clone(&public_calls),
        Arc::clone(&protected_calls),
    );
    let filters = filters_for(&server, None);

    // Single call, with and without a (necessarily meaningless) header.
    let single = br#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 1}"#;
    for header in [None, Some(basic_header("testuser", "testpass"))] {
        let (response, unauthorized) = process_body(&filters, header.as_deref(), single, None);
        assert!(unauthorized);
        assert_eq!(
            error_code(&response.expect("response")),
            Some(ACCESS_DENIED)
        );
    }

    // Method names are matched case-insensitively; so is the guard.
    let shouting = br#"{"jsonrpc": "2.0", "method": "SECRET", "params": [], "id": 1}"#;
    let (response, unauthorized) = process_body(&filters, None, shouting, None);
    assert!(unauthorized);
    assert_eq!(
        error_code(&response.expect("response")),
        Some(ACCESS_DENIED)
    );

    // Notification: no JSON response, and still no invocation.
    let notification = br#"{"jsonrpc": "2.0", "method": "secret", "params": []}"#;
    let (response, unauthorized) = process_body(&filters, None, notification, None);
    assert!(unauthorized);
    assert!(response.is_none());

    // Mixed batch: the public call is answered, the protected call is denied.
    let batch = br#"[
        {"jsonrpc": "2.0", "method": "ping", "params": [], "id": 1},
        {"jsonrpc": "2.0", "method": "secret", "params": [], "id": 2},
        {"jsonrpc": "2.0", "method": "secret", "params": []}
    ]"#;
    let (response, unauthorized) = process_body(&filters, None, batch, None);
    assert!(unauthorized);
    let response = response.expect("response");
    let entries = response.as_array().expect("batch array");
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].get("result").and_then(Value::as_str),
        Some("pong")
    );
    assert_eq!(entries[0].get("id").and_then(Value::as_i64), Some(1));
    assert_eq!(error_code(&entries[1]), Some(ACCESS_DENIED));
    assert_eq!(entries[1].get("id").and_then(Value::as_i64), Some(2));

    assert_eq!(public_calls.load(Ordering::SeqCst), 1);
    assert_eq!(protected_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_method_over_http_is_refused_without_a_challenge() {
    let public_calls = Arc::new(AtomicUsize::new(0));
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let server = build_server_with_protected_counter(
        Arc::clone(&public_calls),
        Arc::clone(&protected_calls),
    );
    let routes = routes_for(&server, None);

    let post = warp::test::request()
        .method("POST")
        .path("/")
        .body(r#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(post.status(), StatusCode::UNAUTHORIZED);
    // Nothing is configured, so there is nothing a client could supply.
    assert!(post.headers().get(WWW_AUTHENTICATE).is_none());
    let body: Value = serde_json::from_slice(post.body()).expect("json body");
    assert_eq!(error_code(&body), Some(ACCESS_DENIED));

    let get = warp::test::request()
        .method("GET")
        .path("/?jsonrpc=2.0&id=1&method=secret&params=%5B%5D")
        .reply(&routes)
        .await;
    assert_eq!(get.status(), StatusCode::UNAUTHORIZED);
    assert!(get.headers().get(WWW_AUTHENTICATE).is_none());
    let body: Value = serde_json::from_slice(get.body()).expect("json body");
    assert_eq!(error_code(&body), Some(ACCESS_DENIED));

    // Public methods stay open when nothing is configured.
    let public = warp::test::request()
        .method("GET")
        .path("/?jsonrpc=2.0&id=1&method=ping&params=%5B%5D")
        .reply(&routes)
        .await;
    assert_eq!(public.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(public.body()).expect("json body");
    assert_eq!(body.get("result").and_then(Value::as_str), Some("pong"));

    assert_eq!(public_calls.load(Ordering::SeqCst), 1);
    assert_eq!(protected_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_method_over_http_runs_with_valid_credentials() {
    let public_calls = Arc::new(AtomicUsize::new(0));
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let server = build_server_with_protected_counter(
        Arc::clone(&public_calls),
        Arc::clone(&protected_calls),
    );
    let routes = routes_for(&server, Some(test_credentials()));
    let header = basic_header("testuser", "testpass");

    let post = warp::test::request()
        .method("POST")
        .path("/")
        .header("authorization", &header)
        .body(r#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(post.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(post.body()).expect("json body");
    assert_eq!(
        body.get("result").and_then(Value::as_str),
        Some("classified")
    );

    let get = warp::test::request()
        .method("GET")
        .path("/?jsonrpc=2.0&id=1&method=secret&params=%5B%5D")
        .header("authorization", &header)
        .reply(&routes)
        .await;
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(protected_calls.load(Ordering::SeqCst), 2);

    // Credentials are configured, so a missing header is challenged.
    let missing = warp::test::request()
        .method("POST")
        .path("/")
        .body(r#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert!(missing.headers().get(WWW_AUTHENTICATE).is_some());
    assert_eq!(protected_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_password_authenticates_public_methods_but_not_protected_ones() {
    let public_calls = Arc::new(AtomicUsize::new(0));
    let protected_calls = Arc::new(AtomicUsize::new(0));
    let server = build_server_with_protected_counter(
        Arc::clone(&public_calls),
        Arc::clone(&protected_calls),
    );
    let auth = BasicAuth::from_settings(&RpcServerConfig {
        rpc_user: "testuser".to_string(),
        rpc_pass: String::new(),
        ..Default::default()
    })
    .expect("a non-blank username still enables global authentication");
    let routes = routes_for(&server, Some(auth));
    let header = basic_header("testuser", "");

    // Global authentication is unchanged: the empty password is accepted for
    // public methods and a missing header is challenged.
    let public = warp::test::request()
        .method("POST")
        .path("/")
        .header("authorization", &header)
        .body(r#"{"jsonrpc": "2.0", "method": "ping", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(public.status(), StatusCode::OK);
    assert_eq!(public_calls.load(Ordering::SeqCst), 1);

    let missing = warp::test::request()
        .method("POST")
        .path("/")
        .body(r#"{"jsonrpc": "2.0", "method": "ping", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert!(missing.headers().get(WWW_AUTHENTICATE).is_some());

    // An empty password is not a secret, so it cannot unlock protected
    // methods; the client already authenticated, so no challenge is sent.
    let protected = warp::test::request()
        .method("POST")
        .path("/")
        .header("authorization", &header)
        .body(r#"{"jsonrpc": "2.0", "method": "secret", "params": [], "id": 1}"#)
        .reply(&routes)
        .await;
    assert_eq!(protected.status(), StatusCode::UNAUTHORIZED);
    assert!(protected.headers().get(WWW_AUTHENTICATE).is_none());
    let body: Value = serde_json::from_slice(protected.body()).expect("json body");
    assert_eq!(error_code(&body), Some(ACCESS_DENIED));

    assert_eq!(public_calls.load(Ordering::SeqCst), 1);
    assert_eq!(protected_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_rejects_invalid_auth_header() {
    let auth = Arc::new(Some(BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    }));
    let (_server, filters) = build_filters_with_auth(auth, false);
    let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

    let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:wrongpass"));
    let (response, unauthorized) = process_body(&filters, Some(&header), body, None);
    assert!(unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(RpcError::access_denied().code() as i64)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_rejects_missing_auth_header() {
    let auth = Arc::new(Some(BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    }));
    let (_server, filters) = build_filters_with_auth(auth, false);
    let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(RpcError::access_denied().code() as i64)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_accepts_valid_auth_header() {
    let auth = Arc::new(Some(BasicAuth {
        user: b"testuser".to_vec(),
        pass: b"testpass".to_vec(),
    }));
    let (_server, filters) = build_filters_with_auth(auth, false);
    let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

    let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:testpass"));
    let (response, unauthorized) = process_body(&filters, Some(&header), body, None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let obj = response.as_object().expect("response object");
    assert!(obj.get("error").is_none());
    assert!(obj.get("result").is_some());
    assert_eq!(obj.get("id").and_then(Value::as_i64), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn process_body_returns_internal_error_on_panic() {
    let (_server, filters) = build_filters_with_panic_handler();
    let body = br#"{"jsonrpc": "2.0", "method": "panic", "params": [], "id": 1}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(RpcError::internal_server_error().code() as i64)
    );
}

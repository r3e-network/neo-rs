use super::cors::verify_basic_auth;
use super::handlers::process_body;
use super::*;
use crate::server::middleware::{RateLimitCheckResult, RateLimitTier};
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::RpcHandler;
use crate::server::rpc_server_blockchain::RpcServerBlockchain;
use crate::server::rpc_server_node::RpcServerNode;
use crate::server::rpc_server_settings::RpcServerConfig;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::neo_io::BinaryWriter;
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::wallets::KeyPair;
use neo_core::WitnessScope;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use parking_lot::RwLock;
use std::sync::Arc;
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

fn build_filters_with_auth(
    auth: Arc<Option<BasicAuth>>,
    include_wallet: bool,
) -> (Arc<RwLock<RpcServer>>, RpcFilters) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    if include_wallet {
        server.register_handlers(
            crate::server::rpc_server_wallet::RpcServerWallet::register_handlers(),
        );
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
    store: &mut neo_core::persistence::StoreCache,
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

#[tokio::test(flavor = "multi_thread")]
async fn process_body_allows_wallet_method_without_auth_config() {
    let (_server, filters) = build_filters_with_auth(Arc::new(None), true);
    let body = br#"{"jsonrpc": "2.0", "method": "getnewaddress", "params": [], "id": 1}"#;

    let (response, unauthorized) = process_body(&filters, None, body, None);
    assert!(!unauthorized);

    let response = response.expect("response");
    let error = response
        .get("error")
        .and_then(Value::as_object)
        .expect("error object");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(RpcError::no_opened_wallet().code() as i64)
    );
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
    assert!(!unauthorized);

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

#[tokio::test(flavor = "multi_thread")]
async fn http_response_escapes_like_csharp_javascript_encoder() {
    // C# RPC responses serialize via `JToken.ToString()` -> `Utf8JsonWriter`
    // with `JavaScriptEncoder.Default`, which escapes `<` `>` `&` `'` `+` `` ` ``
    // and every non-ASCII code point as `\uXXXX` (surrogate pairs for astral
    // chars), while leaving plain ASCII untouched.
    let body = success_response(
        Some(Value::from(1)),
        Value::String("<a> & 'b' + `c` 中😀".to_string()),
    );

    let response = build_http_response(Some(body), false, false);
    let bytes = warp::hyper::body::to_bytes(response.into_body())
        .await
        .expect("aggregate body");
    let text = String::from_utf8(bytes.to_vec()).expect("ascii-only body is valid utf8");

    assert_eq!(
        text,
        "{\"jsonrpc\":\"2.0\",\"result\":\"\\u003Ca\\u003E \\u0026 \\u0027b\\u0027 \\u002B \\u0060c\\u0060 \\u4E2D\\uD83D\\uDE00\",\"id\":1}"
    );
}

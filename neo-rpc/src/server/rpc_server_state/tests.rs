use super::*;
use neo_config::ProtocolSettings;
use neo_crypto::mpt_trie::MptStoreSnapshot;
use neo_state_service::StateRoot;
use serde_json::json;
use std::collections::HashMap;

use crate::server::rpc_server::RpcServer;

fn make_server_with_state() -> (Arc<neo_system::Node>, Arc<StateStore>, RpcServer) {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let state_store = Arc::new(StateStore::new());
    system.register_service(Arc::clone(&state_store));
    let mut server = RpcServer::new(Arc::clone(&system), Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    (system, state_store, server)
}

fn make_server_without_state() -> RpcServer {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let mut server = RpcServer::new(system, Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    server
}

fn call(server: &RpcServer, method: &str, params: &[Value]) -> Result<Value, RpcException> {
    let handler = server
        .handlers_guard()
        .get(&method.to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| panic!("handler {method} registered"));
    (handler.callback())(server, params)
}

fn seed_state_root(state_store: &StateStore, index: u32, byte: u8) -> StateRoot {
    let root = StateRoot::new_current(index, neo_primitives::UInt256::from([byte; 32]));
    assert!(state_store.try_add_state_root(root.clone()));
    state_store.commit_validated_state_roots(std::slice::from_ref(&root));
    root
}

/// In-memory MPT snapshot used to build real tries for the
/// `verifyproof` round-trip.
#[derive(Default)]
struct MemoryMptStore {
    data: parking_lot::Mutex<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MptStoreSnapshot for MemoryMptStore {
    fn try_get(&self, key: &[u8]) -> neo_crypto::mpt_trie::MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn state_handlers_error_when_service_not_registered() {
    let server = make_server_without_state();
    for method in [
        "getstateheight",
        "getstateroot",
        "getproof",
        "getstate",
        "findstates",
    ] {
        let params = [json!(0)];
        let err = call(&server, method, &params).expect_err("service missing");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::internal_server_error().code(),
            "{method} should fail without a registered state store"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn state_height_reports_null_then_index() {
    let (_system, state_store, server) = make_server_with_state();

    let result = call(&server, "getstateheight", &[]).expect("getstateheight");
    assert_eq!(result.get("localrootindex"), Some(&Value::Null));
    assert_eq!(result.get("validatedrootindex"), Some(&Value::Null));

    seed_state_root(&state_store, 7, 0xAB);
    let result = call(&server, "getstateheight", &[]).expect("getstateheight");
    assert_eq!(result.get("localrootindex"), Some(&json!(7)));
    assert_eq!(result.get("validatedrootindex"), Some(&json!(7)));
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_returns_committed_root() {
    let (_system, state_store, server) = make_server_with_state();
    let root = seed_state_root(&state_store, 3, 0xCD);

    let result = call(&server, "getstateroot", &[json!(3)]).expect("getstateroot");
    assert_eq!(result.get("index"), Some(&json!(3)));
    assert_eq!(
        result.get("roothash").and_then(Value::as_str),
        Some(root.root_hash.to_string().as_str())
    );
    assert_eq!(
        result
            .get("witnesses")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_rejects_unknown_index() {
    let (_system, _state_store, server) = make_server_with_state();
    let err = call(&server, "getstateroot", &[json!(42)]).expect_err("unknown root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_state_root().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn proof_handlers_report_unsupported_state() {
    let (_system, _state_store, server) = make_server_with_state();
    let root = neo_primitives::UInt256::from([0x11u8; 32]).to_string();
    let contract = neo_primitives::UInt160::zero().to_string();
    let key = BASE64_STANDARD.encode([0u8; 4]);

    for (method, params) in [
        (
            "getproof",
            vec![json!(root.clone()), json!(contract.clone()), json!(key.clone())],
        ),
        (
            "getstate",
            vec![json!(root.clone()), json!(contract.clone()), json!(key.clone())],
        ),
        (
            "findstates",
            vec![json!(root), json!(contract), json!(key)],
        ),
    ] {
        let err = call(&server, method, &params).expect_err("proofs unsupported");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unsupported_state().code(),
            "{method} should report the missing MPT backend"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_round_trips_value() {
    // Build a real MPT, capture a proof, and replay it through the
    // verifyproof handler — the same payload format the C# state
    // service emits.
    let store = Arc::new(MemoryMptStore::default());
    let mut trie = Trie::new(Arc::clone(&store), None, true);
    let key = [0x01u8, 0x02, 0x03, 0x04];
    let value = b"state-value".to_vec();
    trie.put(&key, &value).expect("trie put");
    let root_hash = trie.root_hash().expect("root hash");
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof present");

    let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
    let payload = RpcServerState::encode_proof_payload(&key, &nodes);
    let payload_b64 = BASE64_STANDARD.encode(payload);

    let (_system, _state_store, server) = make_server_with_state();
    let params = [json!(root_hash.to_string()), json!(payload_b64)];
    let result = call(&server, "verifyproof", &params).expect("verifyproof");
    let decoded = BASE64_STANDARD
        .decode(result.as_str().expect("base64 value"))
        .expect("decode value");
    assert_eq!(decoded, value);
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_rejects_invalid_payload() {
    let (_system, _state_store, server) = make_server_with_state();
    let root = neo_primitives::UInt256::from([0x22u8; 32]).to_string();
    let bogus = BASE64_STANDARD.encode([0xFFu8; 2]);
    let err =
        call(&server, "verifyproof", &[json!(root), json!(bogus)]).expect_err("invalid payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_rejects_wrong_root() {
    let store = Arc::new(MemoryMptStore::default());
    let mut trie = Trie::new(Arc::clone(&store), None, true);
    let key = [0x09u8, 0x08];
    trie.put(&key, b"value").expect("trie put");
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof present");
    let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
    let payload = RpcServerState::encode_proof_payload(&key, &nodes);
    let payload_b64 = BASE64_STANDARD.encode(payload);

    let (_system, _state_store, server) = make_server_with_state();
    let wrong_root = neo_primitives::UInt256::from([0x33u8; 32]).to_string();
    let err = call(&server, "verifyproof", &[json!(wrong_root), json!(payload_b64)])
        .expect_err("proof must not verify against a foreign root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

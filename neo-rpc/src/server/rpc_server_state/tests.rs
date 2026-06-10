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

// === MPT-backed state queries (getproof / getstate / findstates) ===

use neo_execution::ContractState;
use neo_manifest::{ContractManifest, NefFile};
use neo_state_service::mpt_store::MptChange;
use neo_vm_rs::OpCode;

/// Contract id of the fixture contract deployed into the state trie.
const FIXTURE_CONTRACT_ID: i32 = 77;

struct MptFixture {
    contract_hash: neo_primitives::UInt160,
    root1: UInt256,
    root2: UInt256,
    server: RpcServer,
}

fn fixture_storage_key(suffix: &[u8]) -> Vec<u8> {
    RpcServerState::storage_key_bytes(FIXTURE_CONTRACT_ID, suffix)
}

fn fixture_put(suffix: &[u8], value: &[u8]) -> MptChange {
    MptChange::Put {
        key: fixture_storage_key(suffix),
        value: value.to_vec(),
    }
}

/// Builds a server whose state store persists an MPT seeded across two
/// "blocks":
///
/// - block 1 deploys the fixture contract record (the
///   `ContractManagement` per-contract entry `getproof`/`getstate`
///   resolve ids through) plus three entries under prefix `0x0A` and
///   one under `0x0B`;
/// - block 2 rewrites `0x0A01`, adds `0x0A04` and deletes `0x0A02`.
fn make_server_with_mpt(full_state: bool) -> MptFixture {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let state_store = Arc::new(StateStore::with_mpt(full_state));
    system.register_service(Arc::clone(&state_store));
    let mpt = state_store.mpt().expect("MPT backend enabled");

    let contract_hash =
        neo_primitives::UInt160::from_bytes(&[0x42u8; 20]).expect("fixture contract hash");
    let contract = ContractState::new(
        FIXTURE_CONTRACT_ID,
        contract_hash,
        NefFile::new("test".to_string(), vec![OpCode::PUSH1.byte()]),
        ContractManifest::new("StateFixture".to_string()),
    );
    let record = contract
        .serialize_contract_record()
        .expect("serialize fixture contract record");
    assert_eq!(
        ContractState::deserialize_contract_record(&record)
            .expect("contract record round-trips")
            .id,
        FIXTURE_CONTRACT_ID
    );

    // ContractManagement(-1) / Prefix_Contract(8) / script hash — the
    // key C# GetHistoricalContractState reads.
    let mut contract_key = (-1i32).to_le_bytes().to_vec();
    contract_key.push(8);
    contract_key.extend_from_slice(&contract_hash.to_bytes());

    let root1 = mpt
        .apply_block_changes(
            1,
            None,
            &[
                MptChange::Put {
                    key: contract_key,
                    value: record,
                },
                fixture_put(&[0x0A, 0x01], b"alpha"),
                fixture_put(&[0x0A, 0x02], b"beta"),
                fixture_put(&[0x0A, 0x03], b"gamma"),
                fixture_put(&[0x0B, 0x01], b"other-prefix"),
            ],
        )
        .expect("block 1 applies");
    let root2 = mpt
        .apply_block_changes(
            2,
            Some(root1),
            &[
                fixture_put(&[0x0A, 0x01], b"alpha-v2"),
                fixture_put(&[0x0A, 0x04], b"delta"),
                MptChange::Delete {
                    key: fixture_storage_key(&[0x0A, 0x02]),
                },
            ],
        )
        .expect("block 2 applies");

    let mut server = RpcServer::new(system, Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    MptFixture {
        contract_hash,
        root1,
        root2,
        server,
    }
}

fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

fn decode_b64_value(value: &Value) -> Vec<u8> {
    BASE64_STANDARD
        .decode(value.as_str().expect("base64 string"))
        .expect("valid base64")
}

#[tokio::test(flavor = "multi_thread")]
async fn get_proof_round_trips_through_verify_proof() {
    let fixture = make_server_with_mpt(true);
    let params = [
        json!(fixture.root2.to_string()),
        json!(fixture.contract_hash.to_string()),
        json!(b64(&[0x0A, 0x03])),
    ];
    let proof = call(&fixture.server, "getproof", &params).expect("getproof");

    // The payload leads with the full storage key (C# WriteVarBytes(skey)).
    let payload = decode_b64_value(&proof);
    let (key, nodes) = RpcServerState::decode_proof_payload(&payload).expect("payload decodes");
    assert_eq!(key, fixture_storage_key(&[0x0A, 0x03]));
    assert!(!nodes.is_empty());

    // And verifies against the root it was issued for.
    let verify_params = [json!(fixture.root2.to_string()), proof];
    let value = call(&fixture.server, "verifyproof", &verify_params).expect("verifyproof");
    assert_eq!(decode_b64_value(&value), b"gamma".to_vec());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_state_serves_current_and_historical_roots() {
    let fixture = make_server_with_mpt(true);
    let contract = json!(fixture.contract_hash.to_string());
    let key = json!(b64(&[0x0A, 0x01]));

    let current = call(
        &fixture.server,
        "getstate",
        &[json!(fixture.root2.to_string()), contract.clone(), key.clone()],
    )
    .expect("getstate at current root");
    assert_eq!(decode_b64_value(&current), b"alpha-v2".to_vec());

    let historical = call(
        &fixture.server,
        "getstate",
        &[json!(fixture.root1.to_string()), contract.clone(), key],
    )
    .expect("getstate at historical root");
    assert_eq!(decode_b64_value(&historical), b"alpha".to_vec());

    // 0x0A04 only exists from block 2 onwards.
    let err = call(
        &fixture.server,
        "getstate",
        &[
            json!(fixture.root1.to_string()),
            contract,
            json!(b64(&[0x0A, 0x04])),
        ],
    )
    .expect_err("key absent under the historical root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_reject_unknown_contract() {
    let fixture = make_server_with_mpt(true);
    let foreign = neo_primitives::UInt160::from_bytes(&[0x99u8; 20]).expect("hash");
    for method in ["getproof", "getstate", "findstates"] {
        let err = call(
            &fixture.server,
            method,
            &[
                json!(fixture.root2.to_string()),
                json!(foreign.to_string()),
                json!(b64(&[0x0A])),
            ],
        )
        .expect_err("contract not deployed");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unknown_contract().code(),
            "{method} must report UnknownContract"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_proof_missing_key_reports_unknown_storage_item() {
    let fixture = make_server_with_mpt(true);
    let err = call(
        &fixture.server,
        "getproof",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A, 0x7F])),
        ],
    )
    .expect_err("storage item absent");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_returns_full_page_with_proofs() {
    let fixture = make_server_with_mpt(true);
    let result = call(
        &fixture.server,
        "findstates",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A])),
        ],
    )
    .expect("findstates");

    assert_eq!(result.get("truncated"), Some(&Value::Bool(false)));
    let results = result
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    let keys: Vec<Vec<u8>> = results
        .iter()
        .map(|entry| decode_b64_value(entry.get("key").expect("key")))
        .collect();
    let values: Vec<Vec<u8>> = results
        .iter()
        .map(|entry| decode_b64_value(entry.get("value").expect("value")))
        .collect();
    assert_eq!(
        keys,
        vec![
            vec![0x0A, 0x01],
            vec![0x0A, 0x03],
            vec![0x0A, 0x04],
        ],
        "result keys must strip the contract id and come in trie order"
    );
    assert_eq!(
        values,
        vec![b"alpha-v2".to_vec(), b"gamma".to_vec(), b"delta".to_vec()]
    );

    // firstProof verifies to the first returned value.
    let first_proof = result.get("firstProof").expect("firstProof present").clone();
    let value = call(
        &fixture.server,
        "verifyproof",
        &[json!(fixture.root2.to_string()), first_proof],
    )
    .expect("first proof verifies");
    assert_eq!(decode_b64_value(&value), b"alpha-v2".to_vec());
    assert!(result.get("lastProof").is_some(), "lastProof for >1 results");
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_truncates_and_resumes() {
    let fixture = make_server_with_mpt(true);
    let root = json!(fixture.root2.to_string());
    let contract = json!(fixture.contract_hash.to_string());
    let prefix = json!(b64(&[0x0A]));

    // Page 1: two of three entries -> truncated.
    let page1 = call(
        &fixture.server,
        "findstates",
        &[root.clone(), contract.clone(), prefix.clone(), Value::Null, json!(2)],
    )
    .expect("findstates page 1");
    assert_eq!(page1.get("truncated"), Some(&Value::Bool(true)));
    let results1 = page1
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results1.len(), 2);
    assert!(page1.get("lastProof").is_some());

    // Page 2: resume strictly after the last returned key.
    let resume_key = results1[1].get("key").expect("resume key").clone();
    let page2 = call(
        &fixture.server,
        "findstates",
        &[root.clone(), contract.clone(), prefix.clone(), resume_key],
    )
    .expect("findstates page 2");
    assert_eq!(page2.get("truncated"), Some(&Value::Bool(false)));
    let results2 = page2
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results2.len(), 1);
    assert_eq!(
        decode_b64_value(results2[0].get("key").expect("key")),
        vec![0x0A, 0x04]
    );
    assert!(page2.get("firstProof").is_some(), "single result still proves first");
    assert!(
        page2.get("lastProof").is_none(),
        "lastProof omitted for a single-entry page"
    );

    // A count that exactly matches the remaining entries is not truncated.
    let exact = call(
        &fixture.server,
        "findstates",
        &[root, contract, prefix, Value::Null, json!(3)],
    )
    .expect("findstates exact count");
    assert_eq!(exact.get("truncated"), Some(&Value::Bool(false)));
    assert_eq!(
        exact
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_rejects_from_key_outside_prefix() {
    let fixture = make_server_with_mpt(true);
    let err = call(
        &fixture.server,
        "findstates",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A])),
            json!(b64(&[0x0B, 0x01])),
        ],
    )
    .expect_err("from key must extend the prefix");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_gate_on_current_root_without_full_state() {
    let fixture = make_server_with_mpt(false);
    let contract = json!(fixture.contract_hash.to_string());
    let key = json!(b64(&[0x0A, 0x01]));

    // The current root stays queryable.
    let current = call(
        &fixture.server,
        "getstate",
        &[json!(fixture.root2.to_string()), contract.clone(), key.clone()],
    )
    .expect("current root passes the gate");
    assert_eq!(decode_b64_value(&current), b"alpha-v2".to_vec());

    // Historical roots are rejected with UnsupportedState (C# CheckRootHash).
    for method in ["getproof", "getstate", "findstates"] {
        let err = call(
            &fixture.server,
            method,
            &[json!(fixture.root1.to_string()), contract.clone(), key.clone()],
        )
        .expect_err("historical root must be rejected without FullState");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unsupported_state().code(),
            "{method} must gate on the current root"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_report_resolution_failure_for_unknown_root() {
    let fixture = make_server_with_mpt(true);
    let unknown_root = neo_primitives::UInt256::from([0x77u8; 32]);
    let err = call(
        &fixture.server,
        "getstate",
        &[
            json!(unknown_root.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A, 0x01])),
        ],
    )
    .expect_err("root never persisted");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
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

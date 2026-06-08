use super::*;
use neo_io::BinaryWriter;
use neo_storage::persistence::StorageItem;
use neo_execution::ContractState;
use neo_manifest::ContractManifest;
use neo_native_contracts::NativeRegistry;
use neo_state_service::state_store::MemoryStateStoreBackend;
use neo_state_service::state_store::StateServiceSettings;
use neo_primitives::{NeoSystem, ProtocolSettings};

fn make_server_with_state(full_state: bool) -> (Arc<NeoSystem>, Arc<StateStore>, RpcServer) {
    let settings = ProtocolSettings::default();
    let system = NeoSystem::new_with_state_service(
        settings,
        None,
        None,
        Some(StateServiceSettings {
            full_state,
            ..StateServiceSettings::default()
       }),
    )
    .expect("NeoSystem::new_with_state_service should succeed");

    let state_store = Arc::new(StateStore::new(
        Arc::new(MemoryStateStoreBackend::new()),
        StateServiceSettings {
            full_state,
            ..StateServiceSettings::default()
       },
    ));
    system
        .add_named_service::<StateStore, _>(STATE_STORE_SERVICE, state_store.clone())
        .expect("override state store service");

    let mut server = RpcServer::new(system.clone(), Default::default());
    server.register_handlers(RpcServerState::register_handlers());

    (system, state_store, server)
}

fn store_contract_state(
    store: &mut neo_storage::persistence::StoreCache,
    contract: &ContractState,
) -> (StorageKey, Vec<u8>) {
    const PREFIX_CONTRACT: u8 = 0x08;
    const PREFIX_CONTRACT_HASH: u8 = 0x0c;

    let contract_mgmt_id = NativeRegistry::new()
        .get_by_name("ContractManagement")
        .expect("contract management")
        .id();

    let mut writer = BinaryWriter::new();
    contract.serialize(&mut writer).expect("serialize contract");
    let contract_bytes = writer.into_bytes();

    let mut key_bytes = Vec::with_capacity(1 + 20);
    key_bytes.push(PREFIX_CONTRACT);
    key_bytes.extend_from_slice(&contract.hash.to_bytes());
    let key = StorageKey::new(contract_mgmt_id, key_bytes);
    store.add(key.clone(), StorageItem::from_bytes(contract_bytes.clone()));

    let mut id_bytes = Vec::with_capacity(1 + 4);
    id_bytes.push(PREFIX_CONTRACT_HASH);
    id_bytes.extend_from_slice(&contract.id.to_be_bytes());
    let id_key = StorageKey::new(contract_mgmt_id, id_bytes);
    store.add(
        id_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );

    let mut legacy_bytes = Vec::with_capacity(1 + 4);
    legacy_bytes.push(PREFIX_CONTRACT_HASH);
    legacy_bytes.extend_from_slice(&contract.id.to_le_bytes());
    let legacy_key = StorageKey::new(contract_mgmt_id, legacy_bytes);
    store.add(
        legacy_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );

    store.commit();
    (key, contract_bytes)
}

fn seed_state_root(
    state_store: &StateStore,
    index: u32,
    entries: Vec<(StorageKey, Vec<u8>)>,
) -> UInt256 {
    let mut snapshot = state_store.snapshot();
    for (key, value) in entries {
        snapshot
            .trie
            .put(&key.to_array(), &value)
            .expect("trie put");
   }
    let root_hash = snapshot.trie.root_hash().unwrap_or_else(UInt256::zero);
    let state_root = StateRoot::new_current(index, root_hash);
    snapshot
        .add_local_state_root(&state_root)
        .expect("add local state root");
    snapshot.commit().expect("commit snapshot");
    root_hash
}

#[test]
fn proof_round_trips() {
    let key = vec![1, 2, 3, 4];
    let nodes = vec![vec![0xAA, 0xBB], vec![0xCC]];
    let encoded = StateStore::encode_proof_payload(&key, &nodes);
    let (decoded_key, decoded_nodes) =
        StateStore::decode_proof_payload(&encoded).expect("proof decode");
    assert_eq!(decoded_key, key);
    assert_eq!(decoded_nodes, nodes);
}

#[tokio::test(flavor = "multi_thread")]
async fn state_height_and_root_handlers_work() {
    let (_system, state_store, server) = make_server_with_state(true);

    // Seed a local state root at height 1 via snapshot helpers so current snapshot updates.
    state_store
        .update_local_state_root_snapshot(1, std::iter::empty())
        .expect("stage local state root");
    state_store
        .update_local_state_root(1)
        .expect("commit local state root");
    let root_hash = state_store
        .current_local_root_hash()
        .unwrap_or_else(UInt256::zero);

    // getstateheight
    let height = RpcServerState::get_state_height(&server, &[])
        .expect("state height")
        .as_object()
        .cloned()
        .expect("object");
    assert_eq!(
        height.get("localrootindex").and_then(Value::as_u64),
        Some(1)
    );

    // getstateroot
    let root_obj = RpcServerState::get_state_root(&server, &[Value::Number(1u64.into())])
        .expect("state root")
        .as_object()
        .cloned()
        .expect("object");
    assert_eq!(
        root_obj
            .get("roothash")
            .and_then(Value::as_str)
            .expect("roothash"),
        root_hash.to_string()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_reject_old_roots_when_full_state_disabled() {
    let (_system, state_store, server) = make_server_with_state(false);

    // Install a state store with FullState disabled and seed a current local root at height 1.
    state_store
        .update_local_state_root_snapshot(1, std::iter::empty())
        .expect("stage local state root");
    state_store
        .update_local_state_root(1)
        .expect("commit local state root");

    // Querying a non-current root should return UnsupportedState when FullState is false.
    let old_root = UInt256::from_bytes(&[1u8; 32]).expect("old root hash");
    let params = vec![
        Value::String(old_root.to_string()),
        Value::String("0x0000000000000000000000000000000000000000".to_string()),
        Value::String(BASE64_STANDARD.encode([0u8])),
    ];
    let err = RpcServerState::get_state(&server, &params).unwrap_err();
    assert_eq!(err.code(), RpcError::unsupported_state().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn proof_handlers_round_trip_value() {
    let (_system, state_store, server) = make_server_with_state(true);

    // Seed a storage item in the trie and materialize a state root at height 1.
    let user_key = vec![0x01u8];
    let storage_key = StorageKey::new(1, user_key.clone());
    let value = vec![0xBA, 0xAD, 0xF0, 0x0D];
    let mut snapshot = state_store.snapshot();
    snapshot
        .trie
        .put(&storage_key.as_bytes(), &value)
        .expect("trie put");
    let root_hash = snapshot
        .trie
        .root_hash()
        .expect("root hash should compute after put");
    let state_root = StateRoot::new_current(1, root_hash);
    snapshot
        .add_local_state_root(&state_root)
        .expect("add local state root");
    snapshot.commit().expect("commit snapshot");

    // Build a proof directly from the snapshot and verify it via the handler.
    let proof_nodes: Vec<Vec<u8>> = snapshot
        .trie
        .try_get_proof(&storage_key.as_bytes())
        .expect("proof exists")
        .expect("proof set present")
        .into_iter()
        .collect();
    let encoded_proof = StateStore::encode_proof_payload(&storage_key.as_bytes(), &proof_nodes);
    let decoded_value = RpcServerState::verify_proof(
        &server,
        &[
            Value::String(root_hash.to_string()),
            Value::String(BASE64_STANDARD.encode(&encoded_proof)),
        ],
    )
    .expect("proof verification");
    let decoded = BASE64_STANDARD
        .decode(decoded_value.as_str().expect("verify returns string"))
        .expect("base64 decode");
    assert_eq!(decoded, value);
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_rejects_unknown_index() {
    let (_system, _state_store, server) = make_server_with_state(true);
    let err = RpcServerState::get_state_root(&server, &[Value::Number(999u64.into())])
        .expect_err("unknown state root");
    assert_eq!(err.code(), RpcError::unknown_state_root().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_proof_rejects_invalid_key() {
    let (_system, _state_store, server) = make_server_with_state(true);
    let params = vec![
        Value::String(UInt256::zero().to_string()),
        Value::String(UInt160::zero().to_string()),
        Value::String("invalid_base64".to_string()),
    ];
    let err = RpcServerState::get_proof(&server, &params).expect_err("invalid key");
    assert_eq!(err.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_rejects_invalid_payload() {
    let (_system, _state_store, server) = make_server_with_state(true);
    let params = vec![
        Value::String(UInt256::zero().to_string()),
        Value::String("invalid_proof".to_string()),
    ];
    let err = RpcServerState::verify_proof(&server, &params).expect_err("invalid proof");
    assert_eq!(err.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_state_returns_value_from_trie() {
    let (system, state_store, server) = make_server_with_state(true);
    let script_hash = UInt160::from_bytes(&[0x11u8; 20]).expect("script hash");

    let contract = ContractState::new(
        1,
        script_hash,
        neo_execution::NefFile::new("test".to_string(), vec![0x01]),
        ContractManifest::default(),
    );
    let mut store = system.context().store_snapshot_cache();
    let (contract_key, contract_bytes) = store_contract_state(&mut store, &contract);

    let storage_key = StorageKey::new(1, vec![0x01, 0x02]);
    let value = vec![0xaa, 0xbb];
    let root_hash = seed_state_root(
        state_store.as_ref(),
        1,
        vec![(contract_key, contract_bytes), (storage_key, value.clone())],
    );

    let params = vec![
        Value::String(root_hash.to_string()),
        Value::String(script_hash.to_string()),
        Value::String(BASE64_STANDARD.encode([0x01u8, 0x02u8])),
    ];
    let result = RpcServerState::get_state(&server, &params)
        .expect("state value")
        .as_str()
        .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
        .expect("string");
    assert_eq!(result, value);
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_returns_results() {
    let (system, state_store, server) = make_server_with_state(true);
    let script_hash = UInt160::from_bytes(&[0x22u8; 20]).expect("script hash");

    let contract = ContractState::new(
        1,
        script_hash,
        neo_execution::NefFile::new("test".to_string(), vec![0x01]),
        ContractManifest::default(),
    );
    let mut store = system.context().store_snapshot_cache();
    let (contract_key, contract_bytes) = store_contract_state(&mut store, &contract);

    let key1 = StorageKey::new(1, vec![0x01, 0x02]);
    let key2 = StorageKey::new(1, vec![0x03, 0x04]);
    let root_hash = seed_state_root(
        state_store.as_ref(),
        1,
        vec![
            (contract_key, contract_bytes),
            (key1, vec![0xaa, 0xbb]),
            (key2, vec![0xcc, 0xdd]),
        ],
    );

    let params = vec![
        Value::String(root_hash.to_string()),
        Value::String(script_hash.to_string()),
        Value::String(String::new()),
    ];
    let result = RpcServerState::find_states(&server, &params)
        .expect("find states")
        .as_object()
        .cloned()
        .expect("object");
    let results = result
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    assert_eq!(results.len(), 2);

    let first_key = results[0]
        .get("key")
        .and_then(Value::as_str)
        .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
        .expect("key");
    let first_value = results[0]
        .get("value")
        .and_then(Value::as_str)
        .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
        .expect("value");
    let second_key = results[1]
        .get("key")
        .and_then(Value::as_str)
        .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
        .expect("key");
    let second_value = results[1]
        .get("value")
        .and_then(Value::as_str)
        .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
        .expect("value");

    assert_eq!(first_key, vec![0x01, 0x02]);
    assert_eq!(first_value, vec![0xaa, 0xbb]);
    assert_eq!(second_key, vec![0x03, 0x04]);
    assert_eq!(second_value, vec![0xcc, 0xdd]);
    assert_eq!(result.get("truncated"), Some(&Value::Bool(false)));
}

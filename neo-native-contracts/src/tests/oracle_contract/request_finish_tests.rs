use super::super::*;
use crate::test_support::deploy_native as deploy_contract;
use neo_config::ProtocolSettings;
use neo_execution::Contract;
use neo_execution::contract_state::ContractState;
use neo_execution::native_contract::build_native_contract_state;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
    ContractPermission, ManifestFeatures, NefFile, WildCardContainer,
};
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, BlockHeader, OracleResponse, Transaction, TransactionAttribute};
use neo_primitives::{
    CallFlags, ContractParameterType, OracleResponseCode, TriggerType, UInt160, UInt256,
    Verifiable, WitnessScope,
};
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::{StackItem, script_builder::ScriptBuilder};
use neo_vm_rs::{ExecutionEngineLimits, OpCode, VmState};
use num_bigint::BigInt;
use std::sync::Arc;

/// Builds a tiny deployed contract with one `method(params)` descriptor,
/// so `ContractManagement.IsContract` passes and the queued `finish`
/// callback can resolve a real method. Methods with parameters open with
/// `INITSLOT` (as compiled contracts do) to consume the pushed arguments.
fn mock_contract_state(hash: UInt160, method: &str, params: usize) -> ContractState {
    let script = if params > 0 {
        vec![
            OpCode::INITSLOT.byte(),
            0,
            u8::try_from(params).expect("param count"),
            OpCode::RET.byte(),
        ]
    } else {
        vec![OpCode::RET.byte()]
    };
    let nef = NefFile::new("test".to_string(), script);
    let parameters = (0..params)
        .map(|i| {
            ContractParameterDefinition::new(format!("arg{i}"), ContractParameterType::Any)
                .expect("parameter")
        })
        .collect();
    let descriptor = ContractMethodDescriptor::new(
        method.to_string(),
        parameters,
        ContractParameterType::Void,
        0,
        false,
    )
    .expect("descriptor");
    let manifest = ContractManifest {
        name: "MockOracleClient".to_string(),
        groups: Vec::new(),
        features: ManifestFeatures::empty(),
        supported_standards: Vec::new(),
        abi: ContractAbi::new(vec![descriptor], Vec::new()),
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::default(),
        extra: None,
    };
    ContractState::new(1, hash, nef, manifest)
}

/// Entry script: `OracleContract.request(url, filter, callback, userData, gas)`
/// via System.Contract.Call (args pushed in reverse so arg0 is on top).
fn request_script(
    url: &[u8],
    filter: Option<&[u8]>,
    callback: &[u8],
    user_data: i64,
    gas_for_response: i64,
) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(gas_for_response); // arg4
    builder.emit_push_int(user_data); // arg3 (Any)
    builder.emit_push(callback); // arg2
    match filter {
        Some(f) => {
            builder.emit_push(f); // arg1
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL); // arg1 = null
        }
    }
    builder.emit_push(url); // arg0
    builder.emit_push_int(5);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(b"request");
    builder.emit_push(&OracleContract::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

/// Entry script: `OracleContract.finish()` (zero args).
fn finish_script() -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(b"finish");
    builder.emit_push(&OracleContract::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

fn signed_tx(signer: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn run(script: Vec<u8>, tx: Transaction, snapshot: Arc<DataCache>) -> (VmState, Vec<u8>) {
    crate::install();
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    let names: Vec<u8> = Vec::new();
    let _ = names;
    (state, Vec::new())
}

fn seed_initialized_oracle_storage(cache: &DataCache) {
    OracleContract::new().write_request_id(cache, &BigInt::from(0));
    cache.add(
        OracleContract::price_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_ORACLE_PRICE,
        ))),
    );
}

/// Seeds a snapshot with the Oracle native contract record installed.
fn oracle_snapshot() -> Arc<DataCache> {
    crate::install();
    let cache = DataCache::new(false);
    deploy_contract(
        &cache,
        &build_native_contract_state(&OracleContract, &ProtocolSettings::default(), 0),
    );
    seed_initialized_oracle_storage(&cache);
    Arc::new(cache)
}

#[test]
fn request_writes_record_id_list_counter_and_mints_response_gas() {
    let snapshot = oracle_snapshot();
    let url = b"https://example.org/data";
    let script = request_script(url, Some(b"$.value"), b"cb", 42, 1_0000000);

    // The entry script itself must be a deployed contract for
    // ContractManagement.IsContract(CallingScriptHash) to pass.
    let caller_hash = UInt160::from_script(&script);
    deploy_contract(&snapshot, &mock_contract_state(caller_hash, "dummy", 0));

    let tx = signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap());
    let expected_txid = tx.hash();

    crate::install();
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("script loads");
    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "request must HALT"
    );

    // Request-id counter incremented to 1.
    assert_eq!(OracleContract::new().read_request_id(&snapshot).unwrap(), 1);

    // The stored OracleRequest record (id 0) matches the C# layout.
    let request = OracleContract::new()
        .read_request(&snapshot, 0)
        .unwrap()
        .expect("request stored");
    assert_eq!(request.original_tx_id, expected_txid);
    assert_eq!(request.gas_for_response, 1_0000000);
    assert_eq!(request.url, "https://example.org/data");
    assert_eq!(request.filter, Some("$.value".to_string()));
    assert_eq!(request.callback_contract, caller_hash);
    assert_eq!(request.callback_method, "cb");
    assert_eq!(
        request.user_data,
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default()
        )
        .unwrap()
    );

    // The per-url id-list holds the new id.
    let list_item = snapshot
        .get(&OracleContract::id_list_key("https://example.org/data"))
        .expect("id list written");
    assert_eq!(
        OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
        vec![0]
    );

    // gasForResponse was minted to the Oracle account (GAS Struct[balance]).
    let oracle_addr = OracleContract::script_hash();
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &oracle_addr).unwrap(),
        BigInt::from(1_0000000)
    );

    // The OracleRequest notification carries [id, caller, url, filter].
    let event = engine
        .notifications()
        .iter()
        .find(|n| n.event_name == "OracleRequest")
        .expect("OracleRequest notification");
    assert_eq!(event.script_hash, OracleContract::script_hash());
    assert_eq!(event.state[0].as_int().unwrap(), BigInt::from(0));
    assert_eq!(event.state[1].as_bytes().unwrap(), caller_hash.to_bytes());
    assert_eq!(event.state[2].as_bytes().unwrap(), url.to_vec());
    assert_eq!(event.state[3].as_bytes().unwrap(), b"$.value".to_vec());
}

#[test]
fn second_request_takes_the_next_id() {
    let snapshot = oracle_snapshot();
    let url = b"https://example.org/data";
    let script = request_script(url, None, b"cb", 1, 1_0000000);
    let caller_hash = UInt160::from_script(&script);
    deploy_contract(&snapshot, &mock_contract_state(caller_hash, "dummy", 0));

    for expected_counter in 1..=2u64 {
        let (state, _) = run(
            script.clone(),
            signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
            Arc::clone(&snapshot),
        );
        assert_eq!(state, VmState::HALT);
        assert_eq!(
            OracleContract::new().read_request_id(&snapshot).unwrap(),
            expected_counter
        );
    }
    // Both ids are pending for the url, and a null filter round-trips.
    let list_item = snapshot
        .get(&OracleContract::id_list_key("https://example.org/data"))
        .unwrap();
    assert_eq!(
        OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
        vec![0, 1]
    );
    assert_eq!(
        OracleContract::new()
            .read_request(&snapshot, 1)
            .unwrap()
            .unwrap()
            .filter,
        None
    );
}

#[test]
fn request_validation_faults() {
    let long_url = vec![b'a'; MAX_URL_LENGTH + 1];
    let long_filter = vec![b'f'; MAX_FILTER_LENGTH + 1];
    let long_callback = vec![b'c'; MAX_CALLBACK_LENGTH + 1];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "url too long",
            request_script(&long_url, None, b"cb", 1, 1_0000000),
        ),
        (
            "filter too long",
            request_script(b"https://x", Some(&long_filter), b"cb", 1, 1_0000000),
        ),
        (
            "callback too long",
            request_script(b"https://x", None, &long_callback, 1, 1_0000000),
        ),
        (
            "callback starts with underscore",
            request_script(b"https://x", None, b"_cb", 1, 1_0000000),
        ),
        (
            "gasForResponse below 0.1 GAS",
            request_script(b"https://x", None, b"cb", 1, 9999999),
        ),
    ];
    for (name, script) in cases {
        let snapshot = oracle_snapshot();
        let (state, _) = run(
            script,
            signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
            Arc::clone(&snapshot),
        );
        assert_eq!(state, VmState::FAULT, "{name} must FAULT");
        assert_eq!(
            OracleContract::new().read_request_id(&snapshot).unwrap(),
            0,
            "{name}: no id allocated"
        );
    }
}

#[test]
fn request_from_a_non_contract_caller_faults() {
    // No ContractManagement record for the entry script hash.
    let snapshot = oracle_snapshot();
    let script = request_script(b"https://x", None, b"cb", 1, 1_0000000);
    let (state, _) = run(
        script,
        signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
        Arc::clone(&snapshot),
    );
    assert_eq!(state, VmState::FAULT);
    assert!(
        OracleContract::new()
            .read_request(&snapshot, 0)
            .unwrap()
            .is_none()
    );
}

fn seeded_finish_snapshot(id: u64) -> (Arc<DataCache>, OracleRequest, UInt160) {
    let snapshot = oracle_snapshot();
    let callback_hash = UInt160::from_bytes(&[0xCB; 20]).unwrap();
    deploy_contract(
        &snapshot,
        &mock_contract_state(callback_hash, "oracleCallback", 4),
    );
    let request = OracleRequest::new(
        UInt256::from_bytes(&[0xAA; 32]).unwrap(),
        1_0000000,
        "https://example.org/data",
        Some("$.value".to_string()),
        callback_hash,
        "oracleCallback",
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default(),
        )
        .unwrap(),
    );
    snapshot.add(
        OracleContract::request_key(id),
        StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
    );
    snapshot.add(
        OracleContract::id_list_key(&request.url),
        StorageItem::from_bytes(OracleContract::encode_id_list(&[id]).unwrap()),
    );
    (snapshot, request, callback_hash)
}

fn oracle_response_tx(id: u64, result: &[u8]) -> Transaction {
    let mut tx = signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap());
    tx.add_attribute(TransactionAttribute::OracleResponse(OracleResponse::new(
        id,
        OracleResponseCode::Success,
        result.to_vec(),
    )));
    tx
}

#[test]
fn finish_notifies_and_queues_the_callback() {
    let (snapshot, request, _) = seeded_finish_snapshot(7);
    let tx = oracle_response_tx(7, b"\"abc\"");

    crate::install();
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");
    engine
        .load_script(finish_script(), CallFlags::ALL, None)
        .expect("script loads");
    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "finish must HALT"
    );

    // C# Finish emits OracleResponse [id, originalTxid] before the callback.
    let event = engine
        .notifications()
        .iter()
        .find(|n| n.event_name == "OracleResponse")
        .expect("OracleResponse notification");
    assert_eq!(event.script_hash, OracleContract::script_hash());
    assert_eq!(event.state[0].as_int().unwrap(), BigInt::from(7));
    assert_eq!(
        event.state[1].as_bytes().unwrap(),
        request.original_tx_id.to_bytes()
    );

    // C# Finish does NOT remove the request — PostPersist does.
    assert!(
        OracleContract::new()
            .read_request(&snapshot, 7)
            .unwrap()
            .is_some()
    );
    let list_item = snapshot
        .get(&OracleContract::id_list_key(&request.url))
        .unwrap();
    assert_eq!(
        OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
        vec![7]
    );
}

#[test]
fn finish_without_oracle_response_attribute_faults() {
    let (snapshot, _, _) = seeded_finish_snapshot(7);
    let (state, _) = run(
        finish_script(),
        signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
        snapshot,
    );
    assert_eq!(state, VmState::FAULT);
}

#[test]
fn finish_with_unknown_request_id_faults() {
    let (snapshot, _, _) = seeded_finish_snapshot(7);
    let (state, _) = run(finish_script(), oracle_response_tx(99, b""), snapshot);
    assert_eq!(state, VmState::FAULT);
}

#[test]
fn verify_accepts_only_oracle_response_transactions() {
    crate::install();
    let make_engine = |tx: Transaction| {
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new_with_native_contract_provider(
            TriggerType::Application,
            Some(container),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            10_00000000,
            None,
            Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
        )
        .expect("engine builds")
    };
    let contract = OracleContract::new();

    let mut with_attr = make_engine(oracle_response_tx(1, b""));
    assert_eq!(
        contract.invoke(&mut with_attr, "verify", &[]).unwrap(),
        vec![1]
    );

    let mut without_attr = make_engine(signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()));
    assert_eq!(
        contract.invoke(&mut without_attr, "verify", &[]).unwrap(),
        vec![0]
    );
}

fn post_persist_engine(
    snapshot: Arc<DataCache>,
    block_index: u32,
    txs: Vec<Transaction>,
) -> ApplicationEngine {
    crate::install();
    let mut header = BlockHeader::default();
    header.set_index(block_index);
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::PostPersist,
        None,
        snapshot,
        Some(Block::from_parts(header, txs)),
        ProtocolSettings::default(),
        2000_00000000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds")
}

#[test]
fn post_persist_removes_answered_requests_and_id_list_entries() {
    let (snapshot, request, _) = seeded_finish_snapshot(7);
    // A second pending request for the same url keeps the list alive.
    snapshot.add(
        OracleContract::request_key(8),
        StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
    );
    snapshot.update(
        OracleContract::id_list_key(&request.url),
        StorageItem::from_bytes(OracleContract::encode_id_list(&[7, 8]).unwrap()),
    );

    let mut engine =
        post_persist_engine(Arc::clone(&snapshot), 10, vec![oracle_response_tx(7, b"")]);
    NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");

    assert!(
        OracleContract::new()
            .read_request(&snapshot, 7)
            .unwrap()
            .is_none(),
        "request removed"
    );
    assert!(
        OracleContract::new()
            .read_request(&snapshot, 8)
            .unwrap()
            .is_some(),
        "other request kept"
    );
    let list_item = snapshot
        .get(&OracleContract::id_list_key(&request.url))
        .expect("list kept");
    assert_eq!(
        OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
        vec![8]
    );

    // Answering the last pending id deletes the list entry entirely.
    let mut engine =
        post_persist_engine(Arc::clone(&snapshot), 11, vec![oracle_response_tx(8, b"")]);
    NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");
    assert!(
        OracleContract::new()
            .read_request(&snapshot, 8)
            .unwrap()
            .is_none()
    );
    assert!(
        snapshot
            .get(&OracleContract::id_list_key(&request.url))
            .is_none(),
        "empty list deleted"
    );

    // A response without a stored request is skipped (no fault).
    let mut engine =
        post_persist_engine(Arc::clone(&snapshot), 12, vec![oracle_response_tx(9, b"")]);
    NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");
}

#[test]
fn post_persist_mints_the_price_to_the_designated_oracle_node() {
    use neo_crypto::ECPoint;

    let (snapshot, _, _) = seeded_finish_snapshot(7);
    // Designate one oracle node at index 0 (RoleManagement layout:
    // (id, [role_byte, index_be4]) -> BinarySerialized Array[pubkey]).
    let pubkey = ECPoint::from_bytes(
        &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap(),
    )
    .unwrap();
    let role_key = crate::RoleManagement::designation_key(crate::Role::Oracle.as_byte(), 0);
    let nodes = BinarySerializer::serialize(
        &StackItem::from_array(vec![StackItem::from_byte_string(pubkey.to_bytes())]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    snapshot.add(role_key, StorageItem::from_bytes(nodes));

    let mut engine =
        post_persist_engine(Arc::clone(&snapshot), 10, vec![oracle_response_tx(7, b"")]);
    NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");

    // The node received the default 0.5 GAS oracle price.
    let node_account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey));
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &node_account).unwrap(),
        BigInt::from(DEFAULT_ORACLE_PRICE)
    );
}

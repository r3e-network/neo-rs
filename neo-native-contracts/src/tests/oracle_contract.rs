use super::*;
use super::request::OracleIdList;
use crate::test_support::deploy_native as deploy_contract;
use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_execution::contract_state::ContractState;
use neo_execution::native_contract::build_native_contract_state;
use neo_manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
    ContractPermission, NefFile, WildCardContainer,
};
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, BlockHeader, OracleResponse, TransactionAttribute};
use neo_primitives::{
    CallFlags, ContractParameterType, OracleResponseCode, TriggerType, UInt256, Verifiable,
    WitnessScope,
};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::Interoperable;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, StackValue, VmState};
use std::sync::Arc;

// ===== from oracle_native_tests.rs =====
#[test]
fn native_contract_surface() {
    let c = OracleContract::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["getPrice", "setPrice", "request", "finish", "verify"]
    );

    let setter = c.methods().iter().find(|m| m.name == "setPrice").unwrap();
    assert!(!setter.safe);
    assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(setter.return_type, ContractParameterType::Void);

    let request = c.methods().iter().find(|m| m.name == "request").unwrap();
    assert!(!request.safe);
    assert_eq!(request.cpu_fee, 0);
    assert_eq!(
        request.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        request.parameters,
        vec![
            ContractParameterType::String,
            ContractParameterType::String,
            ContractParameterType::String,
            ContractParameterType::Any,
            ContractParameterType::Integer,
        ]
    );
    assert_eq!(request.return_type, ContractParameterType::Void);

    let finish = c.methods().iter().find(|m| m.name == "finish").unwrap();
    assert!(!finish.safe);
    assert_eq!(finish.cpu_fee, 0);
    assert_eq!(
        finish.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert!(finish.parameters.is_empty());
    assert_eq!(finish.return_type, ContractParameterType::Void);

    let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
    // C# ContractMethodMetadata: Safe = (RequiredCallFlags & ~ReadOnly) == 0,
    // and Verify declares CallFlags.None.
    assert!(verify.safe);
    assert_eq!(verify.cpu_fee, 1 << 15);
    assert_eq!(verify.required_call_flags, CallFlags::NONE.bits());
    assert!(verify.parameters.is_empty());
    assert_eq!(verify.return_type, ContractParameterType::Boolean);
}

#[test]
fn set_price_write_round_trips() {
    let cache = DataCache::new(false);
    cache.add(
        OracleContract::price_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_ORACLE_PRICE,
        ))),
    );
    // The setter's storage effect (overwrite Prefix_Price) is observed by
    // the getter's reader.
    OracleContract::new()
        .put_price(&cache, 7_5000000)
        .expect("initialized price can be overwritten"); // 0.75 GAS
    assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 7_5000000);
}

#[test]
fn price_requires_initialized_storage() {
    let cache = DataCache::new(false);
    assert!(OracleContract::new().read_price(&cache).is_err());
    assert!(OracleContract::new().put_price(&cache, 12345678).is_err());

    let key = OracleContract::price_key();
    cache.add(
        key,
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(12345678))),
    );
    assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 12345678);
}

fn sample_request(filter: Option<String>) -> OracleRequest {
    OracleRequest::new(
        UInt256::from_bytes(&[0xAA; 32]).unwrap(),
        1_0000000,
        "https://example.org/data",
        filter,
        UInt160::from_bytes(&[0xCB; 20]).unwrap(),
        "oracleCallback",
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default(),
        )
        .unwrap(),
    )
}

#[test]
fn request_record_round_trips() {
    for filter in [Some("$.value".to_string()), None] {
        let request = sample_request(filter);
        let bytes = OracleContract::encode_oracle_request(&request).unwrap();
        let decoded = OracleContract::decode_oracle_request(&bytes).unwrap();
        assert_eq!(decoded, request);
    }
}

#[test]
fn request_record_add_rejects_duplicate_id_like_csharp() {
    // C# Request writes the record via SnapshotCache.Add, so a reused
    // request id faults and must not overwrite the existing request.
    let cache = DataCache::new(false);
    let contract = OracleContract::new();
    let original = sample_request(None);
    contract
        .add_request_record(&cache, 5, &original)
        .expect("first add succeeds");
    let replacement = sample_request(Some("$.replacement".to_string()));
    let err = contract
        .add_request_record(&cache, 5, &replacement)
        .expect_err("duplicate request id must fault");
    assert!(err.to_string().contains("duplicate request id 5"), "{err}");
    assert_eq!(contract.read_request(&cache, 5).unwrap(), Some(original));
}

#[test]
fn request_record_layout_matches_csharp_to_stack_item() {
    // C# OracleRequest.ToStackItem: an Array of 7 items —
    // [txid bytes, Integer, url, filter|Null, contract bytes, method, userdata].
    let request = sample_request(Some("$.x".to_string()));
    let bytes = OracleContract::encode_oracle_request(&request).unwrap();
    let expected_item = StackItem::from_array(vec![
        StackItem::from_byte_string(request.original_tx_id.to_bytes()),
        StackItem::from_int(BigInt::from(request.gas_for_response)),
        StackItem::from_byte_string(request.url.as_bytes().to_vec()),
        StackItem::from_byte_string(b"$.x".to_vec()),
        StackItem::from_byte_string(request.callback_contract.to_bytes()),
        StackItem::from_byte_string(request.callback_method.as_bytes().to_vec()),
        StackItem::from_byte_string(request.user_data.clone()),
    ]);
    let expected =
        BinarySerializer::serialize(&expected_item, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(
        Interoperable::to_stack_value(&request).unwrap(),
        StackValue::try_from(expected_item.clone()).unwrap()
    );

    let mut trait_decoded = OracleRequest::default();
    Interoperable::from_stack_value(
        &mut trait_decoded,
        StackValue::try_from(expected_item).unwrap(),
    )
    .unwrap();
    assert_eq!(trait_decoded, request);

    let item =
        BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
    let StackItem::Array(array) = item else {
        panic!("OracleRequest must serialize as an Array (not Struct)");
    };
    let items = array.items();
    assert_eq!(items.len(), 7);
    assert_eq!(items[0].as_bytes().unwrap(), vec![0xAA; 32]);
    assert_eq!(items[1].as_int().unwrap(), BigInt::from(1_0000000));
    assert_eq!(
        items[2].as_bytes().unwrap(),
        b"https://example.org/data".to_vec()
    );
    assert_eq!(items[3].as_bytes().unwrap(), b"$.x".to_vec());
    assert_eq!(items[4].as_bytes().unwrap(), vec![0xCB; 20]);
    assert_eq!(items[5].as_bytes().unwrap(), b"oracleCallback".to_vec());
    assert_eq!(
        items[6].as_bytes().unwrap(),
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default()
        )
        .unwrap()
    );

    // A null filter serializes as StackItem::Null in slot 3.
    let no_filter = OracleContract::encode_oracle_request(&sample_request(None)).unwrap();
    let StackItem::Array(array) =
        BinarySerializer::deserialize(&no_filter, &ExecutionEngineLimits::default(), None).unwrap()
    else {
        panic!("array expected");
    };
    assert!(matches!(array.items()[3], StackItem::Null));
}

#[test]
fn oracle_storage_codecs_use_stack_value_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let storage_source = include_str!("../oracle_contract/storage.rs");
    let request_encoder = slice_between(
        storage_source,
        "fn encode_oracle_request",
        "fn decode_oracle_request",
    );
    assert!(request_encoder.contains("to_stack_value"));
    assert!(request_encoder.contains("serialize_stack_value_default"));
    assert!(!request_encoder.contains("StackItem::from_array"));
    assert!(!request_encoder.contains("BinarySerializer::serialize("));

    let request_decoder = slice_between(
        storage_source,
        "fn decode_oracle_request",
        "fn encode_id_list",
    );
    assert!(request_decoder.contains("deserialize_stack_value_with_limits"));
    assert!(request_decoder.contains("OracleRequest::from_stack_value"));
    assert!(!request_decoder.contains("BinarySerializer::deserialize("));

    let id_list_encoder = slice_between(storage_source, "fn encode_id_list", "fn decode_id_list");
    assert!(id_list_encoder.contains("OracleIdList::new"));
    assert!(id_list_encoder.contains("to_stack_value"));
    assert!(!id_list_encoder.contains("StackValue::Array"));
    assert!(id_list_encoder.contains("serialize_stack_value_default"));
    assert!(!id_list_encoder.contains("StackItem::from_array"));
    assert!(!id_list_encoder.contains("BinarySerializer::serialize("));

    let id_list_decoder = slice_between(storage_source, "fn decode_id_list", "fn read_request");
    assert!(id_list_decoder.contains("deserialize_stack_value_with_limits"));
    assert!(id_list_decoder.contains("OracleIdList::from_stack_value"));
    assert!(!id_list_decoder.contains("StackValue::Array"));
    assert!(!id_list_decoder.contains("stack_value_as_bigint"));
    assert!(!id_list_decoder.contains("BinarySerializer::deserialize("));

    let source = include_str!("../oracle_contract.rs");
    // C# OracleContract.Request stores `BinarySerializer.Serialize(userData,
    // MaxUserDataLength, engine.Limits.MaxStackSize)` (OracleContract.cs:265).
    // The Rust request path only needs a value projection before reserializing
    // under that byte cap; the later `finish` callback still materializes a
    // StackItem because it queues a VM call.
    let request_user_data = slice_between(
        source,
        "// C#: UserData = BinarySerializer.Serialize(userData,",
        "let request = OracleRequest",
    );
    assert!(request_user_data.contains("deserialize_stack_value_with_limits"));
    assert!(request_user_data.contains("serialize_stack_value_with_limits"));
    assert!(!request_user_data.contains("BinarySerializer::deserialize("));
    assert!(!request_user_data.contains("BinarySerializer::serialize_with_limits("));
}

#[test]
fn invoke_request_args_use_shared_raw_parsers() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let source = include_str!("../oracle_contract.rs");
    let set_price = slice_between(source, "\"setPrice\" =>", "\"request\" =>");
    assert!(set_price.contains("crate::args::raw_i64_arg"));
    assert!(!set_price.contains("BigInt::from_signed_bytes_le(args"));
    assert!(!set_price.contains("BigInt::from_signed_bytes_le(b)"));

    let request = slice_between(source, "\"request\" =>", "\"finish\" =>");
    assert!(request.contains("crate::args::raw_string_arg"));
    assert!(request.contains("crate::args::raw_i64_arg"));
    assert!(!request.contains("String::from_utf8("));
    assert!(!request.contains("BigInt::from_signed_bytes_le(args"));
    assert!(!request.contains("BigInt::from_signed_bytes_le(b)"));
}

#[test]
fn id_list_round_trips_and_key_uses_url_hash160() {
    let ids = vec![0u64, 1, 7, u64::from(u32::MAX) + 5, u64::MAX];
    let bytes = OracleContract::encode_id_list(&ids).unwrap();
    let expected = BinarySerializer::serialize(
        &StackItem::from_array(
            ids.iter()
                .map(|id| StackItem::from_int(BigInt::from(*id)))
                .collect::<Vec<_>>(),
        ),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(OracleContract::decode_id_list(&bytes).unwrap(), ids);

    // C# GetUrlHash = Crypto.Hash160(strict utf8 url) appended to Prefix_IdList.
    let url = "https://example.org/data";
    let key = OracleContract::id_list_key(url);
    let mut expected = vec![PREFIX_ID_LIST];
    expected.extend_from_slice(&Crypto::hash160(url.as_bytes()));
    assert_eq!(key.key(), expected.as_slice());

    // Request key is Prefix_Request ++ big-endian id.
    let rkey = OracleContract::request_key(0x0102030405060708);
    assert_eq!(
        rkey.key(),
        [
            PREFIX_REQUEST,
            0x01,
            0x02,
            0x03,
            0x04,
            0x05,
            0x06,
            0x07,
            0x08
        ]
    );
}

#[test]
fn oracle_id_list_interoperable_projection_matches_csharp_shape() {
    let ids = vec![0u64, 7, u64::MAX];
    let state = OracleIdList::new(ids.clone());
    let expected_value = StackValue::Array(
        0,
        ids.iter()
            .map(|id| StackValue::BigInteger(BigInt::from(*id).to_signed_bytes_le()))
            .collect::<Vec<_>>(),
    );

    let trait_value = Interoperable::to_stack_value(&state).unwrap();
    assert_eq!(trait_value, expected_value);

    let mut parsed = OracleIdList::new(Vec::new());
    Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
    assert_eq!(parsed.into_ids(), ids);
}

#[test]
fn request_id_counter_round_trips() {
    let cache = DataCache::new(false);
    // C# genesis initialization seeds Prefix_RequestId. Later reads use
    // GetAndChange(...), so a missing counter faults instead of inventing 0.
    assert!(OracleContract::new().read_request_id(&cache).is_err());
    OracleContract::new().write_request_id(&cache, &BigInt::from(1));
    assert_eq!(OracleContract::new().read_request_id(&cache).unwrap(), 1);
    OracleContract::new().write_request_id(&cache, &BigInt::from(u64::MAX));
    assert_eq!(
        OracleContract::new().read_request_id(&cache).unwrap(),
        u64::MAX
    );
}

#[test]
fn request_queries_resolve_storage() {
    let cache = DataCache::new(false);
    let contract = OracleContract::new();
    assert!(contract.get_request(&cache, 1).unwrap().is_none());
    assert!(contract.get_requests(&cache).is_empty());
    assert!(
        contract
            .get_requests_by_url(&cache, "https://example.org/data")
            .unwrap()
            .is_empty()
    );

    let request = sample_request(None);
    cache.add(
        OracleContract::request_key(3),
        StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
    );
    cache.add(
        OracleContract::id_list_key(&request.url),
        StorageItem::from_bytes(OracleContract::encode_id_list(&[3]).unwrap()),
    );

    assert_eq!(
        contract.get_request(&cache, 3).unwrap(),
        Some(request.clone())
    );
    assert_eq!(contract.get_requests(&cache), vec![(3, request.clone())]);
    assert_eq!(
        contract.get_requests_by_url(&cache, &request.url).unwrap(),
        vec![(3, request.clone())]
    );

    // The native-contract seam exposes the same record to the engine.
    let details = NativeContract::oracle_request_url_full(&contract, &cache, 3)
        .unwrap()
        .expect("details");
    assert_eq!(details.url, request.url);
    assert_eq!(details.original_tx_id, request.original_tx_id);
}

/// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): no
/// standards before HF_Faun, NEP-30 from the Faun height — and the Faun
/// boundary is a manifest-refresh activation (`Activations => [null,
/// HF_Faun]`, OracleContract.cs:56).
#[test]
fn manifest_standards_gain_nep30_at_faun() {
    use neo_config::{Hardfork, ProtocolSettings};
    use neo_execution::native_contract::build_native_contract_state;

    let unscheduled = build_native_contract_state(&OracleContract, &ProtocolSettings::default(), 0);
    assert!(unscheduled.manifest.supported_standards.is_empty());

    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 10);
    let before = build_native_contract_state(&OracleContract, &settings, 9);
    assert!(before.manifest.supported_standards.is_empty());
    let after = build_native_contract_state(&OracleContract, &settings, 10);
    assert_eq!(after.manifest.supported_standards, ["NEP-30"]);

    assert_eq!(
        NativeContract::activations(&OracleContract),
        &[Hardfork::HfFaun]
    );
}

// ===== from oracle_request_finish_tests.rs =====

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
        features: std::collections::HashMap::new(),
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
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
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
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
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
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        2000_00000000,
        None,
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
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            10_00000000,
            None,
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
    ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        snapshot,
        Some(Block::from_parts(header, txs)),
        ProtocolSettings::default(),
        2000_00000000,
        None,
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

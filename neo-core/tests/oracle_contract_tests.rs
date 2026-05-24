use neo_core::cryptography::{ECCurve, ECPoint, Crypto, Secp256r1Crypto};
use neo_core::ledger::{block::Block, block_header::BlockHeader};
use neo_core::neo_io::BinaryWriter;
use neo_core::neo_vm::StackItem;
use neo_core::network::p2p::payloads::{
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
    transaction::Transaction, transaction_attribute::TransactionAttribute,
};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::script_builder::ScriptBuilder;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::application_engine_contract::NativeArgNullMask;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractManifest, ContractMethodDescriptor,
    ContractParameterDefinition, ContractPermission, WildCardContainer,
};
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, NativeHelpers, OracleContract, Role, RoleManagement,
};
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{Contract, ContractParameterType};
use neo_core::{IVerifiable, UInt160};
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::OpCode;
use num_bigint::BigInt;
use std::sync::Arc;

fn sample_point(byte: u8) -> ECPoint {
    let private_key = {
        let mut bytes = [0u8; 32];
        bytes[31] = byte.max(1);
        bytes
    };
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn serialize_nodes(nodes: &[ECPoint]) -> Vec<u8> {
    let items: Vec<StackItem> = nodes
        .iter()
        .map(|node| StackItem::from_byte_string(node.as_bytes().to_vec()))
        .collect();
    let array = StackItem::from_array(items);
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
        .expect("serialize node list")
}

#[test]
fn oracle_method_and_event_metadata_snapshot() {
    let oracle = OracleContract::new();
    let expected_methods: &[(
        &str,
        i64,
        bool,
        u8,
        &[ContractParameterType],
        ContractParameterType,
        &[&str],
    )] = &[
        (
            "request",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            &[
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::Any,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
            &["url", "filter", "callback", "userData", "gasForResponse"],
        ),
        (
            "getPrice",
            1 << 15,
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            &[],
        ),
        (
            "setPrice",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            &["price"],
        ),
        (
            "finish",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
            &[],
            ContractParameterType::Void,
            &[],
        ),
        (
            "verify",
            1 << 15,
            true,
            0,
            &[],
            ContractParameterType::Boolean,
            &[],
        ),
    ];

    assert_eq!(oracle.id(), -9);
    assert_eq!(
        oracle.hash(),
        UInt160::parse("0xfe924b7cfe89ddd271abaf7210a80a7e11178758").unwrap()
    );
    assert_eq!(oracle.name(), "OracleContract");
    assert_eq!(oracle.methods().len(), expected_methods.len());

    for (method, (name, cpu_fee, safe, flags, parameters, return_type, parameter_names)) in
        oracle.methods().iter().zip(expected_methods.iter())
    {
        assert_eq!(method.name.as_str(), *name);
        assert_eq!(method.cpu_fee, *cpu_fee, "{name}");
        assert_eq!(method.storage_fee, 0, "{name}");
        assert_eq!(method.safe, *safe, "{name}");
        assert_eq!(method.required_call_flags, *flags, "{name}");
        assert_eq!(method.parameters.as_slice(), *parameters, "{name}");
        assert_eq!(&method.return_type, return_type, "{name}");
        assert_eq!(method.active_in, None, "{name}");
        assert_eq!(method.deprecated_in, None, "{name}");
        let actual_names = method
            .parameter_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(actual_names, *parameter_names, "{name}");
    }

    let events = oracle.events(&ProtocolSettings::default_settings(), 0);
    let expected_events: &[(&str, &[(&str, ContractParameterType)])] = &[
        (
            "OracleRequest",
            &[
                ("Id", ContractParameterType::Integer),
                ("RequestContract", ContractParameterType::Hash160),
                ("Url", ContractParameterType::String),
                ("Filter", ContractParameterType::String),
            ],
        ),
        (
            "OracleResponse",
            &[
                ("Id", ContractParameterType::Integer),
                ("OriginalTx", ContractParameterType::Hash256),
            ],
        ),
    ];
    assert_eq!(events.len(), expected_events.len());
    for (event, (name, parameters)) in events.iter().zip(expected_events.iter()) {
        assert_eq!(event.name, *name);
        assert_eq!(event.parameters.len(), parameters.len(), "{name}");
        for (parameter, (parameter_name, parameter_type)) in
            event.parameters.iter().zip(*parameters)
        {
            assert_eq!(parameter.name, *parameter_name, "{name}");
            assert_eq!(&parameter.param_type, parameter_type, "{name}");
        }
    }
}

fn setup_post_persist_engine(snapshot: Arc<DataCache>, block: Block) -> ApplicationEngine {
    let script_container: Arc<dyn IVerifiable> = Arc::new(Transaction::new());
    ApplicationEngine::new(
        TriggerType::PostPersist,
        Some(script_container),
        snapshot,
        Some(block),
        ProtocolSettings::default_settings(),
        200_000_000,
        None,
    )
    .expect("engine")
}

fn default_manifest(name: &str) -> ContractManifest {
    let method = ContractMethodDescriptor::new(
        "callback".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        true,
    )
    .expect("method descriptor");
    let abi = ContractAbi::new(vec![method], Vec::new());

    ContractManifest {
        name: name.to_string(),
        groups: Vec::new(),
        features: Default::default(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::create_wildcard(),
        extra: None,
    }
}

fn callback_manifest(name: &str) -> ContractManifest {
    let callback_method = ContractMethodDescriptor::new(
        "callback".to_string(),
        vec![
            ContractParameterDefinition::new("url".to_string(), ContractParameterType::String)
                .expect("url param"),
            ContractParameterDefinition::new("userData".to_string(), ContractParameterType::Any)
                .expect("userData param"),
            ContractParameterDefinition::new("code".to_string(), ContractParameterType::Integer)
                .expect("code param"),
            ContractParameterDefinition::new(
                "result".to_string(),
                ContractParameterType::ByteArray,
            )
            .expect("result param"),
        ],
        ContractParameterType::Void,
        0,
        false,
    )
    .expect("callback descriptor");
    let callback_event = ContractEventDescriptor::new(
        "Callback".to_string(),
        vec![
            ContractParameterDefinition::new("url".to_string(), ContractParameterType::String)
                .expect("event url param"),
            ContractParameterDefinition::new("userData".to_string(), ContractParameterType::Any)
                .expect("event userData param"),
            ContractParameterDefinition::new("code".to_string(), ContractParameterType::Integer)
                .expect("event code param"),
            ContractParameterDefinition::new(
                "result".to_string(),
                ContractParameterType::ByteArray,
            )
            .expect("event result param"),
        ],
    )
    .expect("callback event");
    let abi = ContractAbi::new(vec![callback_method], vec![callback_event]);

    ContractManifest {
        name: name.to_string(),
        groups: Vec::new(),
        features: Default::default(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::create_wildcard(),
        extra: None,
    }
}

const CONTRACT_MANAGEMENT_ID: i32 = -1;
const PREFIX_CONTRACT: u8 = 8;
const PREFIX_CONTRACT_HASH: u8 = 12;

fn add_contract_to_snapshot(snapshot: &DataCache, contract: &ContractState) {
    let mut writer = BinaryWriter::new();
    contract.serialize(&mut writer).expect("serialize contract");
    let mut contract_key = Vec::with_capacity(1 + UInt160::LENGTH);
    contract_key.push(PREFIX_CONTRACT);
    contract_key.extend_from_slice(&contract.hash.to_bytes());
    let key = StorageKey::new(CONTRACT_MANAGEMENT_ID, contract_key);
    snapshot.add(key, StorageItem::from_bytes(writer.into_bytes()));

    let mut id_key_bytes = Vec::with_capacity(1 + std::mem::size_of::<i32>());
    id_key_bytes.push(PREFIX_CONTRACT_HASH);
    id_key_bytes.extend_from_slice(&contract.id.to_be_bytes());
    let id_key = StorageKey::new(CONTRACT_MANAGEMENT_ID, id_key_bytes);
    snapshot.add(
        id_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );
}

fn make_test_contract(id: i32, name: &str) -> ContractState {
    let nef = NefFile::new(name.to_string(), vec![OpCode::RET.byte()]);
    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, name);
    ContractState::new(id, hash, nef, default_manifest(name))
}

fn make_callback_contract(id: i32, name: &str) -> ContractState {
    let mut script = ScriptBuilder::new();
    script.emit_push_int(4);
    script.emit_pack();
    script.emit_push_string("Callback");
    script
        .emit_syscall("System.Runtime.Notify")
        .expect("notify syscall");
    script.emit_opcode(OpCode::RET);

    let nef = NefFile::new(name.to_string(), script.to_array());
    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, name);
    ContractState::new(id, hash, nef, callback_manifest(name))
}

fn make_request_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
    let script_container: Arc<dyn IVerifiable> = Arc::new(Transaction::new());
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        ProtocolSettings::default_settings(),
        2_000_000_000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load dummy script");
    engine
}

fn make_response_transaction(id: u64, code: OracleResponseCode, result: Vec<u8>) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_script(OracleResponse::get_fixed_script());
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(OracleResponse {
        id,
        code,
        result,
    })]);
    tx
}

fn make_response_engine(snapshot: Arc<DataCache>, tx: Transaction) -> ApplicationEngine {
    let script = tx.script().to_vec();
    let script_container: Arc<dyn IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        ProtocolSettings::default_settings(),
        2_000_000_000,
        None,
    )
    .expect("engine");
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load oracle response script");
    engine
}

fn make_engine_without_container(snapshot: Arc<DataCache>) -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default_settings(),
        2_000_000_000,
        None,
    )
    .expect("engine")
}

fn seed_pending_request(
    snapshot: Arc<DataCache>,
    calling_contract: &ContractState,
    url: &str,
    user_data: StackItem,
) -> Arc<DataCache> {
    add_contract_to_snapshot(snapshot.as_ref(), calling_contract);

    let oracle = OracleContract::new();
    let mut engine = make_request_engine(snapshot);
    engine.set_current_script_hash(Some(oracle.hash()));
    engine.set_calling_script_hash(Some(calling_contract.hash));

    let user_data_bytes =
        BinarySerializer::serialize(&user_data, &ExecutionEngineLimits::default())
            .expect("serialize user data");
    let args = vec![
        url.as_bytes().to_vec(),
        Vec::new(),
        b"callback".to_vec(),
        user_data_bytes,
        10_000_000i64.to_le_bytes().to_vec(),
    ];

    engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect("oracle request should succeed");

    engine.snapshot_cache()
}

fn seed_pending_request_bytes(
    snapshot: Arc<DataCache>,
    calling_contract: &ContractState,
    url: &str,
    user_data_bytes: Vec<u8>,
) -> Arc<DataCache> {
    add_contract_to_snapshot(snapshot.as_ref(), calling_contract);

    let oracle = OracleContract::new();
    let mut engine = make_request_engine(snapshot);
    engine.set_current_script_hash(Some(oracle.hash()));
    engine.set_calling_script_hash(Some(calling_contract.hash));

    let args = vec![
        url.as_bytes().to_vec(),
        Vec::new(),
        b"callback".to_vec(),
        user_data_bytes,
        10_000_000i64.to_le_bytes().to_vec(),
    ];

    engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect("oracle request should succeed");

    engine.snapshot_cache()
}

#[test]
fn oracle_set_price_requires_committee_and_stores_signed_le_bigint_price() {
    let snapshot = Arc::new(DataCache::new(false));
    let oracle = OracleContract::new();
    let new_price = 123_456_789i64;
    let price_bytes = BigInt::from(new_price).to_signed_bytes_le();

    let mut unauthorized = make_request_engine(Arc::clone(&snapshot));
    let err = oracle
        .invoke_method(
            &mut unauthorized,
            "setPrice",
            std::slice::from_ref(&price_bytes),
        )
        .expect_err("setPrice without committee witness should fail");
    assert!(
        err.to_string().contains("Committee authorization"),
        "unexpected error: {err}"
    );
    assert_eq!(
        oracle.get_price(unauthorized.snapshot_cache().as_ref()),
        50_000_000,
        "unauthorized setPrice must not mutate the stored price"
    );

    let mut authorized = make_request_engine(Arc::clone(&snapshot));
    let authorized_snapshot = authorized.snapshot_cache();
    let committee = NativeHelpers::committee_address(
        authorized.protocol_settings(),
        Some(authorized_snapshot.as_ref()),
    );
    authorized.set_calling_script_hash(Some(committee));

    oracle
        .invoke_method(&mut authorized, "setPrice", &[price_bytes])
        .expect("committee witness should allow setPrice");

    assert_eq!(
        oracle.get_price(authorized.snapshot_cache().as_ref()),
        new_price,
        "setPrice must round-trip through signed little-endian BigInteger storage"
    );
}

#[test]
fn oracle_verify_returns_false_without_transaction_container() {
    let oracle = OracleContract::new();
    let mut engine = make_engine_without_container(Arc::new(DataCache::new(false)));

    let result = oracle
        .invoke_method(&mut engine, "verify", &[])
        .expect("verify without tx should return false byte");

    assert_eq!(result, vec![0]);
}

#[test]
fn oracle_verify_accepts_fixed_oracle_response_transaction() {
    let oracle = OracleContract::new();
    let tx = make_response_transaction(0, OracleResponseCode::Success, Vec::new());
    let mut engine = make_response_engine(Arc::new(DataCache::new(false)), tx);

    let result = oracle
        .invoke_method(&mut engine, "verify", &[])
        .expect("fixed oracle response should verify");

    assert_eq!(result, vec![1]);
}

#[test]
fn oracle_finish_rejects_direct_invocation_outside_fixed_response_script() {
    let oracle = OracleContract::new();
    let mut engine = make_request_engine(Arc::new(DataCache::new(false)));

    let err = oracle
        .invoke_method(&mut engine, "finish", &[])
        .expect_err("finish must reject direct native invocation");

    assert!(
        err.to_string().contains("fixed response script"),
        "unexpected error: {err}"
    );
}

#[test]
fn oracle_finish_rejects_fixed_script_without_oracle_response_attribute() {
    let mut tx = Transaction::new();
    tx.set_script(OracleResponse::get_fixed_script());
    let mut engine = make_response_engine(Arc::new(DataCache::new(false)), tx);

    let err = engine
        .execute()
        .expect_err("finish should reject transaction without OracleResponse attribute");

    assert!(
        err.to_string()
            .contains("Oracle response attribute missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn oracle_finish_rejects_unknown_request_before_callback() {
    let tx = make_response_transaction(99, OracleResponseCode::Success, Vec::new());
    let mut engine = make_response_engine(Arc::new(DataCache::new(false)), tx);

    let err = engine
        .execute()
        .expect_err("finish should reject responses for unknown requests");

    assert!(
        err.to_string().contains("Request not found"),
        "unexpected error: {err}"
    );
    let oracle_response_events = engine
        .notifications()
        .iter()
        .filter(|notification| {
            notification.script_hash == OracleContract::new().hash()
                && notification.event_name == "OracleResponse"
        })
        .count();
    assert_eq!(
        oracle_response_events, 0,
        "unknown requests must not emit OracleResponse notifications"
    );
    let callback_events = engine
        .notifications()
        .iter()
        .filter(|notification| notification.event_name == "Callback")
        .count();
    assert_eq!(
        callback_events, 0,
        "unknown requests must not invoke callbacks"
    );
}

#[test]
fn oracle_finish_emits_response_before_rejecting_bad_user_data() {
    let snapshot = Arc::new(DataCache::new(false));
    let callback_contract = make_callback_contract(1, "oracleCallback");
    let oracle = OracleContract::new();
    let snapshot = seed_pending_request_bytes(
        Arc::clone(&snapshot),
        &callback_contract,
        "https://example.com/bad-user-data",
        vec![0xff],
    );

    let tx = make_response_transaction(0, OracleResponseCode::Success, Vec::new());
    let mut engine = make_response_engine(snapshot, tx);
    let err = engine
        .execute()
        .expect_err("finish should reject invalid serialized user data before callback");

    assert!(
        err.to_string().contains("Invalid") || err.to_string().contains("invalid"),
        "unexpected error: {err}"
    );

    let oracle_response_events = engine
        .notifications()
        .iter()
        .filter(|notification| {
            notification.script_hash == oracle.hash() && notification.event_name == "OracleResponse"
        })
        .count();
    let callback_events = engine
        .notifications()
        .iter()
        .filter(|notification| {
            notification.script_hash == callback_contract.hash
                && notification.event_name == "Callback"
        })
        .count();

    assert_eq!(
        oracle_response_events, 1,
        "OracleResponse event must be emitted before userData deserialization"
    );
    assert_eq!(
        callback_events, 0,
        "invalid userData must stop before callback invocation"
    );
}

#[test]
fn oracle_request_requires_registered_calling_contract() {
    let snapshot = Arc::new(DataCache::new(false));
    let oracle = OracleContract::new();
    let mut engine = make_request_engine(Arc::clone(&snapshot));
    engine.set_current_script_hash(Some(oracle.hash()));

    let args = vec![
        b"https://example.com".to_vec(),
        Vec::new(),
        b"callback".to_vec(),
        Vec::new(),
        10_000_000i64.to_le_bytes().to_vec(),
    ];

    let err = engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("oracle request without a contract caller should fail");
    assert!(
        err.to_string().contains("contract"),
        "unexpected error: {err}"
    );
    let engine_snapshot = engine.snapshot_cache();
    assert!(
        oracle
            .get_requests(engine_snapshot.as_ref())
            .expect("get requests")
            .is_empty(),
        "request should not be persisted when no contract caller exists"
    );

    let mut unknown_contract_engine = make_request_engine(Arc::clone(&snapshot));
    unknown_contract_engine.set_current_script_hash(Some(oracle.hash()));
    unknown_contract_engine.set_calling_script_hash(Some(
        UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap(),
    ));
    let err = unknown_contract_engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("oracle request from unregistered contract hash should fail");
    assert!(
        err.to_string().contains("contract"),
        "unexpected error: {err}"
    );
    assert!(
        oracle
            .get_requests(unknown_contract_engine.snapshot_cache().as_ref())
            .expect("get requests")
            .is_empty(),
        "request should not be persisted for an unknown calling contract hash"
    );
}

#[test]
fn oracle_request_escrows_gas_for_response_to_oracle_contract_balance() {
    let snapshot = Arc::new(DataCache::new(false));
    let calling_contract = make_test_contract(1, "oracleRequester");
    add_contract_to_snapshot(snapshot.as_ref(), &calling_contract);

    let oracle = OracleContract::new();
    let gas = GasToken::new();
    let mut engine = make_request_engine(Arc::clone(&snapshot));
    engine.set_current_script_hash(Some(oracle.hash()));
    engine.set_calling_script_hash(Some(calling_contract.hash));

    let args = vec![
        b"https://example.com".to_vec(),
        Vec::new(),
        b"callback".to_vec(),
        b"payload".to_vec(),
        10_000_000i64.to_le_bytes().to_vec(),
    ];

    engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect("oracle request from contract caller should succeed");

    let engine_snapshot = engine.snapshot_cache();
    let request = oracle
        .get_request(engine_snapshot.as_ref(), 0)
        .expect("get request")
        .expect("request should be persisted");
    assert_eq!(request.callback_contract, calling_contract.hash);
    assert_eq!(request.callback_method, "callback");
    assert_eq!(request.user_data, b"payload".to_vec());

    let escrow = gas.balance_of_snapshot(engine_snapshot.as_ref(), &oracle.hash());
    assert_eq!(escrow, BigInt::from(10_000_000));
}

#[test]
fn oracle_request_distinguishes_null_filter_from_empty_filter() {
    let snapshot = Arc::new(DataCache::new(false));
    let calling_contract = make_test_contract(1, "oracleRequester");
    add_contract_to_snapshot(snapshot.as_ref(), &calling_contract);

    let oracle = OracleContract::new();
    let request_args = |url: &[u8], filter: Vec<u8>| {
        vec![
            url.to_vec(),
            filter,
            b"callback".to_vec(),
            b"payload".to_vec(),
            BigInt::from(10_000_000i64).to_signed_bytes_le(),
        ]
    };

    let mut null_filter_engine = make_request_engine(Arc::clone(&snapshot));
    null_filter_engine.set_current_script_hash(Some(oracle.hash()));
    null_filter_engine.set_calling_script_hash(Some(calling_contract.hash));
    null_filter_engine.set_state(NativeArgNullMask(1 << 1));
    let result = null_filter_engine
        .call_native_contract(
            oracle.hash(),
            "request",
            &request_args(b"https://example.com/null", Vec::new()),
        )
        .expect("request with null filter should succeed");
    assert_eq!(result, Vec::<u8>::new());

    let mut empty_filter_engine = make_request_engine(null_filter_engine.snapshot_cache());
    empty_filter_engine.set_current_script_hash(Some(oracle.hash()));
    empty_filter_engine.set_calling_script_hash(Some(calling_contract.hash));
    empty_filter_engine
        .call_native_contract(
            oracle.hash(),
            "request",
            &request_args(b"https://example.com/empty", Vec::new()),
        )
        .expect("request with empty filter should succeed");

    let engine_snapshot = empty_filter_engine.snapshot_cache();
    let null_filter_request = oracle
        .get_request(engine_snapshot.as_ref(), 0)
        .expect("get null-filter request")
        .expect("null-filter request should be persisted");
    let empty_filter_request = oracle
        .get_request(engine_snapshot.as_ref(), 1)
        .expect("get empty-filter request")
        .expect("empty-filter request should be persisted");

    assert_eq!(null_filter_request.filter, None);
    assert_eq!(empty_filter_request.filter, Some(String::new()));
}

#[test]
fn oracle_request_rejects_full_url_list_before_mutating_state() {
    let mut snapshot = Arc::new(DataCache::new(false));
    let calling_contract = make_test_contract(1, "oracleRequester");
    add_contract_to_snapshot(snapshot.as_ref(), &calling_contract);

    let oracle = OracleContract::new();
    let gas = GasToken::new();
    let url = b"https://example.com/full-list";
    let args = vec![
        url.to_vec(),
        Vec::new(),
        b"callback".to_vec(),
        b"payload".to_vec(),
        BigInt::from(10_000_000i64).to_signed_bytes_le(),
    ];

    for _ in 0..256 {
        let mut engine = make_request_engine(Arc::clone(&snapshot));
        engine.set_current_script_hash(Some(oracle.hash()));
        engine.set_calling_script_hash(Some(calling_contract.hash));
        engine
            .call_native_contract(oracle.hash(), "request", &args)
            .expect("request should fill URL pending list");
        snapshot = engine.snapshot_cache();
    }

    assert_eq!(
        oracle
            .get_requests_by_url(snapshot.as_ref(), std::str::from_utf8(url).unwrap())
            .expect("get requests by url")
            .len(),
        256
    );
    let balance_before = gas.balance_of_snapshot(snapshot.as_ref(), &oracle.hash());

    let mut engine = make_request_engine(Arc::clone(&snapshot));
    engine.set_current_script_hash(Some(oracle.hash()));
    engine.set_calling_script_hash(Some(calling_contract.hash));
    let err = engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("full URL pending list should reject the next request");
    assert!(
        err.to_string().contains("too many pending responses"),
        "unexpected error: {err}"
    );

    let failed_snapshot = engine.snapshot_cache();
    assert_eq!(
        gas.balance_of_snapshot(failed_snapshot.as_ref(), &oracle.hash()),
        balance_before,
        "full URL list rejection must not escrow additional GAS"
    );
    assert!(
        oracle
            .get_request(failed_snapshot.as_ref(), 256)
            .expect("get rejected request id")
            .is_none(),
        "full URL list rejection must not persist the rejected request"
    );
    assert_eq!(
        oracle
            .get_requests_by_url(failed_snapshot.as_ref(), std::str::from_utf8(url).unwrap())
            .expect("get requests by url after rejection")
            .len(),
        256,
        "full URL list rejection must not append another pending id"
    );
}

#[test]
fn oracle_request_rejects_invalid_callback_and_gas_without_persisting() {
    let snapshot = Arc::new(DataCache::new(false));
    let calling_contract = make_test_contract(1, "oracleRequester");
    add_contract_to_snapshot(snapshot.as_ref(), &calling_contract);

    let oracle = OracleContract::new();
    let mut engine = make_request_engine(Arc::clone(&snapshot));
    engine.set_current_script_hash(Some(oracle.hash()));
    engine.set_calling_script_hash(Some(calling_contract.hash));

    let mut args = vec![
        b"https://example.com".to_vec(),
        Vec::new(),
        b"_callback".to_vec(),
        b"payload".to_vec(),
        BigInt::from(10_000_000i64).to_signed_bytes_le(),
    ];
    let err = engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("callback names starting with underscore should fail");
    assert!(
        err.to_string().contains("underscore"),
        "unexpected error: {err}"
    );

    args[2] = Vec::new();
    let err = engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("empty callback names should fail");
    assert!(
        err.to_string().contains("Callback name too long"),
        "unexpected error: {err}"
    );

    args[2] = b"callback".to_vec();
    args[4] = BigInt::from(9_999_999i64).to_signed_bytes_le();
    let err = engine
        .call_native_contract(oracle.hash(), "request", &args)
        .expect_err("gas below minimum response gas should fail");
    assert!(
        err.to_string().contains("Invalid gas amount"),
        "unexpected error: {err}"
    );

    assert!(
        oracle
            .get_requests(engine.snapshot_cache().as_ref())
            .expect("get requests")
            .is_empty(),
        "invalid requests must not be persisted"
    );
}

#[test]
fn oracle_finish_invokes_callback_with_url_userdata_code_and_result() {
    let snapshot = Arc::new(DataCache::new(false));
    let callback_contract = make_callback_contract(1, "oracleCallback");
    let oracle = OracleContract::new();
    let url = "https://example.com/api";
    let user_data = StackItem::from_int(42);
    let result = b"{\"price\":42}".to_vec();

    let snapshot = seed_pending_request(
        Arc::clone(&snapshot),
        &callback_contract,
        url,
        user_data.clone(),
    );

    let request_snapshot = Arc::clone(&snapshot);
    let request = oracle
        .get_request(request_snapshot.as_ref(), 0)
        .expect("get request")
        .expect("request should exist before finish");

    let tx = make_response_transaction(0, OracleResponseCode::Success, result.clone());
    let mut engine = make_response_engine(Arc::clone(&snapshot), tx);
    engine.execute().expect("oracle finish execution");

    let oracle_response_events: Vec<_> = engine
        .notifications()
        .iter()
        .filter(|notification| {
            notification.script_hash == oracle.hash() && notification.event_name == "OracleResponse"
        })
        .collect();
    assert_eq!(
        oracle_response_events.len(),
        1,
        "expected one OracleResponse event"
    );
    assert_eq!(
        oracle_response_events[0].state[0]
            .as_int()
            .expect("response id")
            .clone(),
        BigInt::from(0),
    );
    assert_eq!(
        oracle_response_events[0].state[1]
            .as_bytes()
            .expect("original tx bytes"),
        request.original_tx_id.to_bytes(),
    );

    let callback_events: Vec<_> = engine
        .notifications()
        .iter()
        .filter(|notification| {
            notification.script_hash == callback_contract.hash
                && notification.event_name == "Callback"
        })
        .collect();
    assert_eq!(callback_events.len(), 1, "expected one callback event");

    let callback_state = &callback_events[0].state;
    assert_eq!(
        callback_state.len(),
        4,
        "callback must receive four arguments"
    );
    assert_eq!(
        callback_state[0].as_bytes().expect("url bytes"),
        url.as_bytes()
    );
    assert_eq!(
        callback_state[1]
            .as_int()
            .expect("user data integer")
            .clone(),
        BigInt::from(42),
    );
    assert_eq!(
        callback_state[2]
            .as_int()
            .expect("response code integer")
            .clone(),
        BigInt::from(OracleResponseCode::Success as i32),
    );
    assert_eq!(callback_state[3].as_bytes().expect("result bytes"), result);

    let engine_snapshot = engine.snapshot_cache();
    assert!(
        oracle
            .get_request(engine_snapshot.as_ref(), 0)
            .expect("get request after finish")
            .is_some(),
        "finish should not remove the pending request before post-persist"
    );
}

#[test]
fn oracle_post_persist_cleans_known_requests_and_mints_gas_for_designated_nodes() {
    let snapshot = Arc::new(DataCache::new(false));
    let requester = make_test_contract(1, "oracleRequester");
    let url = "https://example.com/post-persist";
    let snapshot = seed_pending_request(
        Arc::clone(&snapshot),
        &requester,
        url,
        StackItem::from_byte_string(b"payload".to_vec()),
    );

    let header = BlockHeader {
        index: 7,
        timestamp: 1_700_000_000,
        ..Default::default()
    };
    let tx = make_response_transaction(0, OracleResponseCode::Success, Vec::new());
    let block = Block::new(header, vec![tx]);
    let mut engine = setup_post_persist_engine(Arc::clone(&snapshot), block);

    let role_contract = RoleManagement::new();
    let oracle_point = sample_point(0xAB);
    let mut suffix = vec![Role::Oracle as u8];
    suffix.extend_from_slice(&7u32.to_be_bytes());
    let key = StorageKey::new(role_contract.id(), suffix);
    let serialized = serialize_nodes(std::slice::from_ref(&oracle_point));
    snapshot.add(key, StorageItem::from_bytes(serialized));
    seed_ledger_current_index(&snapshot, 7);

    let oracle = OracleContract::new();
    engine.set_current_script_hash(Some(oracle.hash()));
    oracle
        .post_persist(&mut engine)
        .expect("post persist succeeds");
    engine.set_current_script_hash(None);

    let engine_snapshot = engine.snapshot_cache();
    assert!(
        oracle
            .get_request(engine_snapshot.as_ref(), 0)
            .expect("get request after post-persist")
            .is_none(),
        "post-persist should remove completed requests"
    );
    assert!(
        oracle
            .get_requests_by_url(engine_snapshot.as_ref(), url)
            .expect("get requests by url")
            .is_empty(),
        "post-persist should clear the URL id list for completed requests"
    );

    let script = Contract::create_signature_redeem_script(oracle_point);
    let account =
        UInt160::from_bytes(&Crypto::hash160(&script)).expect("convert designated account");
    let gas = GasToken::new();
    let balance = gas.balance_of_snapshot(engine_snapshot.as_ref(), &account);
    let expected = BigInt::from(oracle.get_price(engine_snapshot.as_ref()));

    assert_eq!(balance, expected, "designated node should receive reward");
}

#[test]
fn oracle_post_persist_ignores_unknown_response_ids() {
    let snapshot = Arc::new(DataCache::new(false));
    let header = BlockHeader {
        index: 7,
        timestamp: 1_700_000_000,
        ..Default::default()
    };
    let tx = make_response_transaction(42, OracleResponseCode::Success, Vec::new());
    let block = Block::new(header, vec![tx]);
    let mut engine = setup_post_persist_engine(Arc::clone(&snapshot), block);

    let role_contract = RoleManagement::new();
    let oracle_point = sample_point(0xAB);
    let mut suffix = vec![Role::Oracle as u8];
    suffix.extend_from_slice(&7u32.to_be_bytes());
    let key = StorageKey::new(role_contract.id(), suffix);
    let serialized = serialize_nodes(std::slice::from_ref(&oracle_point));
    snapshot.add(key, StorageItem::from_bytes(serialized));
    seed_ledger_current_index(&snapshot, 7);

    let oracle = OracleContract::new();
    engine.set_current_script_hash(Some(oracle.hash()));
    oracle
        .post_persist(&mut engine)
        .expect("post persist succeeds");
    engine.set_current_script_hash(None);

    let script = Contract::create_signature_redeem_script(oracle_point);
    let account =
        UInt160::from_bytes(&Crypto::hash160(&script)).expect("convert designated account");
    let gas = GasToken::new();
    let balance = gas.balance_of_snapshot(engine.snapshot_cache().as_ref(), &account);

    assert_eq!(
        balance,
        BigInt::from(0),
        "unknown response ids must not be rewarded"
    );
}

fn seed_ledger_current_index(snapshot: &Arc<DataCache>, index: u32) {
    const PREFIX_CURRENT_BLOCK: u8 = 12;
    let ledger = LedgerContract::new();
    let key = StorageKey::new(ledger.id(), vec![PREFIX_CURRENT_BLOCK]);
    let mut bytes = vec![0u8; 32];
    bytes.extend_from_slice(&index.to_le_bytes());
    snapshot.add(key, StorageItem::from_bytes(bytes));
}

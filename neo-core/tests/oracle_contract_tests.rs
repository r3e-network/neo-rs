use neo_core::cryptography::{ECCurve, ECPoint, NeoHash, Secp256r1Crypto};
use neo_core::ledger::{block::Block, block_header::BlockHeader};
use neo_core::neo_io::BinaryWriter;
use neo_core::network::p2p::payloads::{
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
    transaction::Transaction, transaction_attribute::TransactionAttribute,
};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractPermission, WildCardContainer,
};
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, OracleContract, Role, RoleManagement,
};
use neo_core::smart_contract::storage_item::StorageItem;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::Contract;
use neo_core::{IVerifiable, UInt160};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::{OpCode, StackItem};
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

fn setup_engine(snapshot: Arc<DataCache>, block: Block) -> ApplicationEngine {
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
        neo_core::smart_contract::ContractParameterType::Void,
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
    let nef = NefFile::new(name.to_string(), vec![OpCode::RET as u8]);
    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, name);
    ContractState::new(id, hash, nef, default_manifest(name))
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
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load dummy script");
    engine
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
fn oracle_post_persist_mints_gas_for_designated_nodes() {
    let snapshot = Arc::new(DataCache::new(false));
    let header = BlockHeader {
        index: 7,
        timestamp: 1_700_000_000,
        ..Default::default()
    };

    let mut tx = Transaction::new();
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(OracleResponse {
        id: 42,
        code: OracleResponseCode::Success,
        result: Vec::new(),
    })]);

    let block = Block::new(header, vec![tx]);
    let engine_snapshot = Arc::clone(&snapshot);
    let mut engine = setup_engine(engine_snapshot, block);

    // Seed RoleManagement storage with a single oracle node designation at height 7.
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

    // GAS should be minted to the script hash derived from the designated node.
    let script = Contract::create_signature_redeem_script(oracle_point);
    let account =
        UInt160::from_bytes(&NeoHash::hash160(&script)).expect("convert designated account");
    let gas = GasToken::new();
    let balance = gas.balance_of_snapshot(snapshot.as_ref(), &account);
    let expected = BigInt::from(oracle.get_price(snapshot.as_ref()));

    assert_eq!(balance, expected, "designated node should receive reward");
}
fn seed_ledger_current_index(snapshot: &Arc<DataCache>, index: u32) {
    const PREFIX_CURRENT_BLOCK: u8 = 12;
    let ledger = LedgerContract::new();
    let key = StorageKey::new(ledger.id(), vec![PREFIX_CURRENT_BLOCK]);
    let mut bytes = vec![0u8; 32];
    bytes.extend_from_slice(&index.to_le_bytes());
    snapshot.add(key, StorageItem::from_bytes(bytes));
}

use neo_core::neo_io::BinaryWriter;
use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::find_options::FindOptions;
use neo_core::smart_contract::iterators::IIterator;
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractPermission, WildCardContainer,
};
use neo_core::smart_contract::storage_context::StorageContext;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::{UInt160, constants};
use neo_vm::{OpCode, StackItem};
use std::sync::Arc;

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

fn make_contract(id: i32, name: &str, script: Vec<u8>) -> ContractState {
    let method = ContractMethodDescriptor::new(
        "dummy".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        true,
    )
    .expect("method");
    let abi = ContractAbi::new(vec![method], Vec::new());
    let manifest = ContractManifest {
        name: name.to_string(),
        groups: Vec::new(),
        features: Default::default(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::create_wildcard(),
        extra: None,
    };
    let nef = NefFile::new(name.to_string(), script);
    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, name);
    ContractState::new(id, hash, nef, manifest)
}

fn make_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        200_000_000,
        None,
    )
    .expect("engine")
}

#[test]
fn storage_contexts_match_contract_id() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract = make_contract(1, "storage", vec![OpCode::RET as u8]);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    let mut engine = make_engine(Arc::clone(&snapshot));
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");

    let context = engine.get_storage_context().expect("context");
    assert_eq!(context.id, contract.id);
    assert!(!context.is_read_only);

    let read_only = engine.get_read_only_storage_context().expect("context");
    assert_eq!(read_only.id, contract.id);
    assert!(read_only.is_read_only);
}

#[test]
fn storage_get_returns_value() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract = make_contract(2, "storage", vec![OpCode::RET as u8]);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    let key = vec![0x01];
    let value = vec![0x01, 0x02, 0x03, 0x04];
    snapshot.add(
        StorageKey::new(contract.id, key.clone()),
        StorageItem::from_bytes(value.clone()),
    );

    let mut engine = make_engine(Arc::clone(&snapshot));
    engine
        .load_script(
            vec![OpCode::RET as u8],
            CallFlags::READ_STATES,
            Some(contract.hash),
        )
        .expect("load script");

    let context = StorageContext::new(contract.id, false);
    let result = engine.storage_get(context, key).expect("get");
    assert_eq!(result, Some(value));
}

#[test]
fn storage_put_validates_sizes_and_readonly() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract = make_contract(3, "storage", vec![OpCode::RET as u8]);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    let mut engine = make_engine(Arc::clone(&snapshot));
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");

    let context = StorageContext::new(contract.id, false);
    let key = vec![0x01];
    let value = vec![0x02];
    engine
        .storage_put(context.clone(), key.clone(), value.clone())
        .expect("put");

    let oversized_key = vec![0u8; constants::MAX_STORAGE_KEY_SIZE + 1];
    let err = engine
        .storage_put(context.clone(), oversized_key, value.clone())
        .expect_err("oversized key");
    assert!(err.contains("Key too large"));

    let oversized_value = vec![0u8; constants::MAX_STORAGE_VALUE_SIZE + 1];
    let err = engine
        .storage_put(context.clone(), key.clone(), oversized_value)
        .expect_err("oversized value");
    assert!(err.contains("Value too large"));

    let read_only = StorageContext::new(contract.id, true);
    let err = engine
        .storage_put(read_only, key, value)
        .expect_err("readonly");
    assert!(err.contains("read-only"));

    let empty_key = vec![0x0A];
    engine
        .storage_put(context.clone(), empty_key.clone(), Vec::new())
        .expect("empty value");
    let stored = engine.storage_get(context, empty_key).expect("get empty");
    assert_eq!(stored, Some(Vec::new()));
}

#[test]
fn storage_delete_rejects_readonly_context() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract = make_contract(4, "storage", vec![OpCode::RET as u8]);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    let mut engine = make_engine(Arc::clone(&snapshot));
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");

    let context = StorageContext::new(contract.id, true);
    let err = engine
        .storage_delete(context, vec![0x01])
        .expect_err("readonly");
    assert!(err.contains("read-only"));
}

#[test]
fn storage_find_values_only_returns_payload() {
    let snapshot = Arc::new(DataCache::new(false));
    let contract = make_contract(5, "storage", vec![OpCode::RET as u8]);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    snapshot.add(
        StorageKey::new(contract.id, vec![0x01]),
        StorageItem::from_bytes(vec![0xAA, 0xBB]),
    );

    let mut engine = make_engine(Arc::clone(&snapshot));
    engine
        .load_script(
            vec![OpCode::RET as u8],
            CallFlags::READ_STATES,
            Some(contract.hash),
        )
        .expect("load script");

    let context = StorageContext::new(contract.id, false);
    let mut iterator = engine
        .storage_find(context, vec![0x01], FindOptions::ValuesOnly)
        .expect("find");
    assert!(iterator.next());
    let StackItem::ByteString(value) = iterator.value() else {
        panic!("expected byte string");
    };
    assert_eq!(value, vec![0xAA, 0xBB]);
}

use super::*;
use crate::neo_io::BinaryWriter;
use crate::network::p2p::payloads::signer::Signer;
use crate::network::p2p::payloads::transaction::Transaction;
use crate::persistence::{DataCache, IReadOnlyStoreGeneric, SeekDirection, StorageItem};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_state::NefFile;
use crate::smart_contract::execution_context_state::ExecutionContextState;
use crate::smart_contract::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use crate::smart_contract::manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractMethodDescriptor,
    ContractParameterDefinition, ContractPermission, WildCardContainer,
};
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::IInteroperable;
use crate::wallets::KeyPair;
use crate::witness::Witness;
use crate::{IVerifiable, UInt160, WitnessScope};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::OpCode;
use neo_vm::StackItem;
use std::sync::Arc;

fn default_manifest() -> ContractManifest {
    let method = ContractMethodDescriptor::new(
        "testMethod".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        true,
    )
    .expect("method descriptor");
    let abi = ContractAbi::new(vec![method], Vec::new());

    ContractManifest {
        name: "testManifest".to_string(),
        groups: Vec::new(),
        features: Default::default(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::create_wildcard(),
        extra: None,
    }
}

fn manifest_bytes(manifest: &ContractManifest) -> Vec<u8> {
    let json = manifest.to_json().expect("manifest json");
    serde_json::to_vec(&json).expect("manifest json bytes")
}

fn make_nef(script: Vec<u8>) -> NefFile {
    NefFile::new(String::new(), script)
}

fn contract_from_bytes(bytes: &[u8]) -> ContractState {
    let item = BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
        .expect("deserialize contract state stack item");
    let mut contract = ContractState::default();
    contract.from_stack_item(item);
    contract
}

fn add_contract_to_snapshot(snapshot: &DataCache, contract: &ContractState) {
    let mut writer = BinaryWriter::new();
    contract.serialize(&mut writer).expect("serialize contract");
    let key = StorageKey::new(
        ContractManagement::ID,
        ContractManagement::contract_storage_key(&contract.hash),
    );
    snapshot.add(key, StorageItem::from_bytes(writer.into_bytes()));

    let id_key = StorageKey::new(
        ContractManagement::ID,
        ContractManagement::contract_id_storage_key(contract.id),
    );
    snapshot.add(
        id_key,
        StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
    );
}

fn make_engine(
    snapshot: Arc<DataCache>,
    sender: Option<UInt160>,
    gas_limit: i64,
) -> ApplicationEngine {
    let container = sender.map(|account| {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
        tx.add_witness(Witness::new());
        Arc::new(tx) as Arc<dyn IVerifiable>
    });

    ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        None,
        Default::default(),
        gas_limit,
        None,
    )
    .expect("engine")
}

#[test]
fn deploy_rejects_missing_sender_and_invalid_payloads() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let nef = make_nef(vec![OpCode::RET as u8; u8::MAX as usize]);
    let nef_bytes = nef.to_bytes();
    let manifest = default_manifest();
    let manifest_payload = manifest_bytes(&manifest);

    let mut no_sender = make_engine(Arc::clone(&snapshot), None, 50_000_000_000);
    let err = no_sender
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes.clone(), manifest_payload.clone(), Vec::new()],
        )
        .expect_err("missing sender should fail");
    assert!(matches!(err, Error::InvalidOperation { .. }));

    let mut oversized_manifest =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let too_large = vec![0u8; MAX_MANIFEST_LENGTH + 1];
    let err = oversized_manifest
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes.clone(), too_large, Vec::new()],
        )
        .expect_err("oversized manifest should fail");
    assert!(matches!(err, Error::InvalidData { .. }));

    let mut empty_nef = make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let err = empty_nef
        .call_native_contract(
            cm_hash,
            "deploy",
            &[Vec::new(), manifest_payload.clone(), Vec::new()],
        )
        .expect_err("empty NEF should fail");
    assert!(matches!(err, Error::InvalidData { .. }));

    let mut empty_manifest =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let err = empty_manifest
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes.clone(), Vec::new(), Vec::new()],
        )
        .expect_err("empty manifest should fail");
    assert!(matches!(err, Error::InvalidData { .. }));

    let mut insufficient_gas =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 10_000_000);
    let err = insufficient_gas
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes, manifest_payload, Vec::new()],
        )
        .expect_err("insufficient gas should fail");
    assert!(matches!(err, Error::InsufficientGas { .. }));
}

#[test]
fn deploy_returns_expected_hash_and_prevents_duplicates() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let nef = make_nef(vec![OpCode::RET as u8; u8::MAX as usize]);
    let nef_bytes = nef.to_bytes();
    let manifest = default_manifest();
    let manifest_payload = manifest_bytes(&manifest);

    let mut engine = make_engine(snapshot, Some(UInt160::zero()), 50_000_000_000);
    let contract_bytes = engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes.clone(), manifest_payload.clone(), Vec::new()],
        )
        .expect("deploy succeeds");

    let contract = contract_from_bytes(&contract_bytes);
    assert_eq!(
        contract.hash.to_hex_string(),
        "0x7b37d4bd3d87f53825c3554bd1a617318235a685"
    );

    let err = engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef_bytes, manifest_payload, Vec::new()],
        )
        .expect_err("duplicate deploy should fail");
    assert!(matches!(err, Error::InvalidOperation { .. }));
}

#[test]
fn update_preserves_storage_and_increments_counter() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();

    let initial_nef = make_nef(vec![OpCode::RET as u8]);
    let manifest = default_manifest();
    let manifest_payload = manifest_bytes(&manifest);

    let mut deploy_engine =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let contract_bytes = deploy_engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[initial_nef.to_bytes(), manifest_payload, Vec::new()],
        )
        .expect("deploy succeeds");
    let contract = contract_from_bytes(&contract_bytes);

    let storage_key = StorageKey::new(contract.id, vec![0x01]);
    snapshot.add(storage_key, StorageItem::from_bytes(vec![0x01]));

    let mut updated_manifest = default_manifest();
    updated_manifest.name = contract.manifest.name.clone();
    let key = KeyPair::new(vec![1u8; 32]).expect("keypair");
    let signature = key.sign(&contract.hash.to_bytes()).expect("signature");
    let pub_key = key.get_public_key_point().expect("pubkey");
    updated_manifest.groups = vec![ContractGroup::new(pub_key, signature)];

    let updated_nef = make_nef(vec![OpCode::NOP as u8, OpCode::RET as u8]);
    let update_payload = vec![
        updated_nef.to_bytes(),
        manifest_bytes(&updated_manifest),
        Vec::new(),
    ];

    let mut update_engine = make_engine(Arc::clone(&snapshot), None, 50_000_000_000);
    update_engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");
    let state = update_engine
        .current_execution_state()
        .expect("execution state");
    {
        let mut state = state.lock();
        state.call_flags = CallFlags::ALL;
        state.native_calling_script_hash = Some(contract.hash);
    }
    update_engine
        .refresh_context_tracking()
        .expect("refresh context");

    update_engine
        .call_native_contract(cm_hash, "update", &update_payload)
        .expect("update succeeds");

    let updated = ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &contract.hash)
        .expect("get contract")
        .expect("contract exists");
    assert_eq!(updated.update_counter, 1);
    assert_eq!(updated.id, contract.id);
    assert_eq!(updated.nef.script, updated_nef.script);
    assert_eq!(updated.manifest, updated_manifest);

    let prefix = StorageKey::new(contract.id, Vec::new());
    let count = snapshot.find(Some(&prefix), SeekDirection::Forward).count();
    assert_eq!(count, 1);
}

#[test]
fn update_requires_calling_context() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let nef = make_nef(vec![OpCode::RET as u8]);
    let method = ContractMethodDescriptor::new(
        "testMethod".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        false,
    )
    .expect("method descriptor");
    let mut manifest = ContractManifest::new("DestroyTarget".to_string());
    manifest.abi = ContractAbi::new(vec![method], Vec::new());
    let manifest_payload = manifest_bytes(&manifest);

    let mut engine = make_engine(snapshot, None, 50_000_000_000);
    let err = engine
        .call_native_contract(
            cm_hash,
            "update",
            &[nef.to_bytes(), manifest_payload, Vec::new()],
        )
        .expect_err("missing calling context should fail");
    assert!(matches!(err, Error::InvalidOperation { .. }));
}

#[test]
fn update_rejects_empty_payloads() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let nef = make_nef(vec![OpCode::RET as u8]);
    let manifest = default_manifest();

    let mut deploy_engine =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let contract_bytes = deploy_engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef.to_bytes(), manifest_bytes(&manifest), Vec::new()],
        )
        .expect("deploy succeeds");
    let contract = contract_from_bytes(&contract_bytes);

    let mut update_engine = make_engine(Arc::clone(&snapshot), None, 50_000_000_000);
    update_engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");
    let state = update_engine
        .current_execution_state()
        .expect("execution state");
    {
        let mut state = state.lock();
        state.native_calling_script_hash = Some(contract.hash);
    }
    update_engine
        .refresh_context_tracking()
        .expect("refresh context");

    let err = update_engine
        .call_native_contract(cm_hash, "update", &[Vec::new(), Vec::new(), Vec::new()])
        .expect_err("empty payloads should fail");
    assert!(matches!(err, Error::InvalidData { .. }));
}

#[test]
fn update_rejects_oversized_manifest() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let nef = make_nef(vec![OpCode::RET as u8]);
    let manifest = default_manifest();

    let mut deploy_engine =
        make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);
    let contract_bytes = deploy_engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef.to_bytes(), manifest_bytes(&manifest), Vec::new()],
        )
        .expect("deploy succeeds");
    let contract = contract_from_bytes(&contract_bytes);

    let mut update_engine = make_engine(Arc::clone(&snapshot), None, 50_000_000_000);
    update_engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");
    let state = update_engine
        .current_execution_state()
        .expect("execution state");
    {
        let mut state = state.lock();
        state.native_calling_script_hash = Some(contract.hash);
    }
    update_engine
        .refresh_context_tracking()
        .expect("refresh context");

    let oversized_manifest = vec![0u8; MAX_MANIFEST_LENGTH + 1];
    let err = update_engine
        .call_native_contract(
            cm_hash,
            "update",
            &[Vec::new(), oversized_manifest, Vec::new()],
        )
        .expect_err("oversized manifest should fail");
    assert!(matches!(err, Error::InvalidData { .. }));
}

#[test]
fn is_contract_and_list_contracts_filter_native() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm = ContractManagement::new();
    let manifest = default_manifest();
    let nef = make_nef(vec![OpCode::RET as u8]);
    let contract = ContractState::new(1, UInt160::zero(), nef, manifest);
    add_contract_to_snapshot(snapshot.as_ref(), &contract);

    let missing = UInt160::from_bytes(&[0x01; 20]).expect("hash");
    assert!(!ContractManagement::is_contract(snapshot.as_ref(), &missing).unwrap());
    assert!(ContractManagement::is_contract(snapshot.as_ref(), &contract.hash).unwrap());

    if let Some(native_state) = cm.contract_state(&Default::default(), 0) {
        add_contract_to_snapshot(snapshot.as_ref(), &native_state);
    }

    let list = ContractManagement::list_contracts(snapshot.as_ref()).expect("list contracts");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].hash, contract.hash);
}

#[test]
fn has_method_accepts_any_parameter_count() {
    let cm = ContractManagement::new();
    let mut manifest = default_manifest();
    manifest.abi = ContractAbi::new(
        vec![ContractMethodDescriptor::new(
            "alpha".to_string(),
            vec![
                ContractParameterDefinition::new("p0".to_string(), ContractParameterType::Any)
                    .unwrap(),
            ],
            ContractParameterType::Void,
            0,
            true,
        )
        .unwrap()],
        Vec::new(),
    );

    let nef = make_nef(vec![OpCode::RET as u8]);
    let contract = ContractState::new(7, UInt160::zero(), nef, manifest);
    {
        let mut storage = cm.storage.write();
        storage.contracts.insert(contract.hash, contract.clone());
        storage.contract_ids.insert(contract.id, contract.hash);
    }

    assert!(cm.has_method(&contract.hash, "alpha", 1).unwrap());
    assert!(cm.has_method(&contract.hash, "alpha", -1).unwrap());
    assert!(!cm.has_method(&contract.hash, "alpha", 2).unwrap());
}

#[test]
fn contract_hashes_sorted_and_non_native() {
    let cm = ContractManagement::new();
    let manifest = default_manifest();
    let nef = make_nef(vec![OpCode::RET as u8]);

    let contract_a = ContractState::new(
        2,
        UInt160::from_bytes(&[0x02; 20]).unwrap(),
        nef.clone(),
        manifest.clone(),
    );
    let contract_b = ContractState::new(
        1,
        UInt160::from_bytes(&[0x01; 20]).unwrap(),
        nef.clone(),
        manifest,
    );
    let native = ContractState::new(
        -5,
        UInt160::from_bytes(&[0xFF; 20]).unwrap(),
        nef,
        default_manifest(),
    );

    {
        let mut storage = cm.storage.write();
        storage
            .contracts
            .insert(contract_a.hash, contract_a.clone());
        storage
            .contracts
            .insert(contract_b.hash, contract_b.clone());
        storage.contracts.insert(native.hash, native.clone());
        storage.contract_ids.insert(contract_a.id, contract_a.hash);
        storage.contract_ids.insert(contract_b.id, contract_b.hash);
        storage.contract_ids.insert(native.id, native.hash);
    }

    let hashes = cm.get_contract_hashes().expect("hashes");
    assert_eq!(hashes, vec![contract_b.hash, contract_a.hash]);
}

#[test]
fn get_contract_hashes_returns_iterator() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();
    let manifest = default_manifest();
    let nef = make_nef(vec![OpCode::RET as u8]);

    let contract_a = ContractState::new(
        1,
        UInt160::from_bytes(&[0x01; 20]).unwrap(),
        nef.clone(),
        manifest.clone(),
    );
    let contract_b = ContractState::new(
        2,
        UInt160::from_bytes(&[0x02; 20]).unwrap(),
        nef.clone(),
        manifest,
    );
    add_contract_to_snapshot(snapshot.as_ref(), &contract_a);
    add_contract_to_snapshot(snapshot.as_ref(), &contract_b);

    let mut engine = make_engine(snapshot, None, 50_000_000_000);
    let result = engine
        .call_native_contract(cm_hash, "getContractHashes", &[])
        .expect("getContractHashes");
    let iterator_id = u32::from_le_bytes(result.as_slice().try_into().expect("iterator id length"));

    let mut hashes = Vec::new();
    while engine
        .iterator_next_internal(iterator_id)
        .expect("iterator next")
    {
        let item = engine
            .iterator_value_internal(iterator_id)
            .expect("iterator value");
        let StackItem::Struct(struct_item) = item else {
            panic!("expected struct item");
        };
        let items = struct_item.items();
        let value_bytes = items[1].as_bytes().expect("value bytes");
        hashes.push(UInt160::from_bytes(&value_bytes).expect("hash"));
    }

    assert_eq!(hashes, vec![contract_a.hash, contract_b.hash]);
}

#[test]
fn destroy_removes_contract_and_storage() {
    let snapshot = Arc::new(DataCache::new(false));
    let cm_hash = ContractManagement::new().hash();

    let mut engine = make_engine(Arc::clone(&snapshot), Some(UInt160::zero()), 50_000_000_000);

    let nef = make_nef(vec![OpCode::RET as u8]);
    let manifest = default_manifest();
    let manifest_payload = manifest_bytes(&manifest);

    let contract_bytes = engine
        .call_native_contract(
            cm_hash,
            "deploy",
            &[nef.to_bytes(), manifest_payload, Vec::new()],
        )
        .expect("deploy");
    let contract = contract_from_bytes(&contract_bytes);

    let storage_key = StorageKey::new(contract.id, vec![0x01]);
    snapshot.add(storage_key, StorageItem::from_bytes(vec![0x01]));

    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    engine
        .call_contract_dynamic(&contract.hash, "testMethod", CallFlags::ALL, Vec::new())
        .expect("call contract");
    for context in engine.invocation_stack() {
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let mut state = state_arc.lock();
        state.call_flags = CallFlags::ALL;
    }
    let state = engine.current_execution_state().expect("execution state");
    state.lock().native_calling_script_hash = Some(contract.hash);
    engine.refresh_context_tracking().expect("refresh context");
    engine
        .call_native_contract(cm_hash, "destroy", &[])
        .expect("destroy");

    let prefix = StorageKey::new(contract.id, Vec::new());
    assert!(snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .next()
        .is_none());

    let contract_key = StorageKey::new(
        ContractManagement::ID,
        ContractManagement::contract_storage_key(&contract.hash),
    );
    assert!(snapshot.get(&contract_key).is_none());
}

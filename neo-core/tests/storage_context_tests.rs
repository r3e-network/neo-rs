use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::persistence::DataCache;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractPermission,
    WildCardContainer,
};
use neo_core::smart_contract::native::ContractManagement;
use neo_core::smart_contract::storage_context::StorageContext;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::IInteroperable;
use neo_core::witness::Witness;
use neo_core::{IVerifiable, NativeContract, UInt160, WitnessScope};
use neo_vm::{ExecutionEngineLimits, OpCode};
use std::sync::Arc;

fn default_manifest() -> ContractManifest {
    let method = ContractMethodDescriptor::new(
        "testMethod".to_string(),
        Vec::new(),
        neo_core::smart_contract::ContractParameterType::Void,
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

fn make_engine(snapshot: Arc<DataCache>, sender: UInt160) -> neo_core::smart_contract::application_engine::ApplicationEngine {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
    tx.add_witness(Witness::new());
    let container: Arc<dyn IVerifiable> = Arc::new(tx);

    neo_core::smart_contract::application_engine::ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        50_000_000_000,
        None,
    )
    .expect("engine")
}

fn deploy_contract(
    engine: &mut neo_core::smart_contract::application_engine::ApplicationEngine,
) -> ContractState {
    let nef = NefFile::new("test".to_string(), vec![OpCode::RET as u8]);
    let manifest = default_manifest();
    let manifest_json = manifest.to_json().expect("manifest json");
    let manifest_bytes = serde_json::to_vec(&manifest_json).expect("manifest bytes");

    let cm_hash = ContractManagement::new().hash();
    let result = engine
        .call_native_contract(cm_hash, "deploy", &[nef.to_bytes(), manifest_bytes, Vec::new()])
        .expect("deploy");

    let contract_item =
        BinarySerializer::deserialize(&result, &ExecutionEngineLimits::default(), None)
            .expect("contract state item");
    let mut contract =
        ContractState::new(0, UInt160::zero(), nef, ContractManifest::new(String::new()));
    contract.from_stack_item(contract_item);
    contract
}

#[test]
fn storage_context_matches_contract_id() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(Arc::clone(&snapshot), UInt160::zero());
    let contract = deploy_contract(&mut engine);

    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");

    let context = engine.get_storage_context().expect("storage context");
    assert_eq!(context.id, contract.id);
    assert!(!context.is_read_only);

    let ro_context = engine.get_read_only_storage_context().expect("readonly context");
    assert_eq!(ro_context.id, contract.id);
    assert!(ro_context.is_read_only);
}

#[test]
fn storage_get_put_delete_roundtrip() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(Arc::clone(&snapshot), UInt160::zero());
    let contract = deploy_contract(&mut engine);

    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");
    let context = StorageContext::new(contract.id, false);

    engine
        .storage_put(context.clone(), vec![0x01], vec![0x02])
        .expect("put");
    let value = engine
        .storage_get(context.clone(), vec![0x01])
        .expect("get");
    assert_eq!(value, Some(vec![0x02]));

    engine
        .storage_delete(context.clone(), vec![0x01])
        .expect("delete");
    let value = engine
        .storage_get(context, vec![0x01])
        .expect("get after delete");
    assert_eq!(value, None);
}

#[test]
fn storage_put_rejects_readonly_context() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(Arc::clone(&snapshot), UInt160::zero());
    let contract = deploy_contract(&mut engine);

    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, Some(contract.hash))
        .expect("load script");
    let context = StorageContext::new(contract.id, true);

    let err = engine
        .storage_put(context, vec![0x01], vec![0x02])
        .expect_err("readonly put should fail");
    assert!(err.contains("read-only"));
}

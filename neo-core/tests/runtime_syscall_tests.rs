use hex::{decode as hex_decode, encode as hex_encode};
use neo_core::cryptography::Secp256r1Crypto;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::ledger::Block;
use neo_core::neo_io::BinaryWriter;
use neo_core::network::p2p::payloads::{Signer, Transaction, WitnessScope};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractManifest, ContractMethodDescriptor,
    ContractParameterDefinition, ContractPermission, WildCardContainer,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::wallets::KeyPair;
use neo_core::{UInt160, UInt256};
use neo_vm::vm_state::VMState;
use neo_vm::{OpCode, ScriptBuilder, StackItem};
use num_traits::ToPrimitive;
use std::sync::Arc;

#[test]
fn runtime_load_script_passes_args_in_reverse_order() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut inner = ScriptBuilder::new();
    inner.emit_opcode(OpCode::SUB);
    inner.emit_opcode(OpCode::RET);
    let inner_script = inner.to_array();

    let mut outer = ScriptBuilder::new();
    outer.emit_push_bytes(&inner_script);
    outer.emit_push_int(i64::from(CallFlags::ALL.bits()));
    outer.emit_push_int(3);
    outer.emit_push_int(10);
    outer.emit_push_int(2);
    outer.emit_opcode(OpCode::PACK);
    outer
        .emit_syscall("System.Runtime.LoadScript")
        .expect("loadscript syscall");
    outer.emit_opcode(OpCode::RET);

    engine
        .load_script(outer.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.state(), VMState::HALT);
    assert_eq!(engine.result_stack().len(), 1);

    let result = engine.result_stack().peek(0).expect("result item");
    let value = result.as_int().expect("int").to_i64().expect("fits i64");
    assert_eq!(value, -7);
}

#[test]
fn runtime_current_signers_returns_transaction_signers() {
    let snapshot = Arc::new(DataCache::new(false));

    let account_bytes = [7u8; 20];
    let account = UInt160::from_bytes(&account_bytes).expect("account");
    let signer = Signer::new(account, WitnessScope::GLOBAL);

    let mut tx = Transaction::new();
    tx.set_signers(vec![signer.clone()]);

    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.CurrentSigners")
        .expect("currentsigners syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::ALL, None)
        .expect("load script");
    let flags = engine.get_current_call_flags().expect("call flags");
    assert!(flags.contains(CallFlags::ALLOW_NOTIFY));
    engine.execute().expect("execute");

    assert_eq!(engine.state(), VMState::HALT);
    assert_eq!(engine.result_stack().len(), 1);

    let result = engine.result_stack().peek(0).expect("result item");
    let StackItem::Array(signers) = result else {
        panic!("expected Array result, got {result:?}");
    };

    let signer_items = signers.items();
    assert_eq!(signer_items.len(), 1);
    let signer_item = &signer_items[0];

    let StackItem::Array(fields) = signer_item else {
        panic!("expected Signer array, got {signer_item:?}");
    };

    let field_items = fields.items();
    assert_eq!(field_items.len(), 5);
    let encoded_account = field_items[0].as_bytes().expect("account bytes");
    assert_eq!(encoded_account, account.to_bytes());

    let scopes = field_items[1]
        .as_int()
        .expect("scopes int")
        .to_u8()
        .expect("scopes fits u8");
    assert_eq!(scopes, WitnessScope::GLOBAL.bits());

    for (index, item) in field_items.iter().enumerate().take(5).skip(2) {
        let StackItem::Array(array) = item else {
            panic!("expected array at {index}, got {item:?}");
        };
        assert!(array.items().is_empty());
    }
}

#[test]
fn runtime_current_signers_returns_null_without_container() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.CurrentSigners")
        .expect("currentsigners syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result item");
    assert!(result.is_null());
}

#[test]
fn runtime_get_script_container_returns_stack_item() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    let account = UInt160::from_bytes(&[1u8; 20]).expect("account");
    tx.add_signer(Signer::new(account, WitnessScope::NONE));
    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx.clone());
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("script container syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result item");
    match result {
        StackItem::Array(array) => {
            assert_eq!(array.len(), 8);
            let items = array.items();
            let hash_item = items[0].as_bytes().expect("hash bytes");
            assert_eq!(hash_item, tx.hash().to_bytes());
        }
        _ => panic!("Expected script container stack item array"),
    }
}

#[test]
fn runtime_get_script_container_faults_without_container() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("script container syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

#[test]
fn runtime_get_trigger_returns_application() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetTrigger")
        .expect("trigger syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_int()
        .expect("int");
    let value = result.to_i64().expect("fits i64");
    assert_eq!(value, i64::from(TriggerType::APPLICATION.bits()));
}

#[test]
fn runtime_get_time_returns_block_timestamp() {
    let snapshot = Arc::new(DataCache::new(false));
    let header = BlockHeader::new(
        0,
        UInt256::default(),
        UInt256::default(),
        42_000,
        0,
        0,
        0,
        UInt160::zero(),
        Vec::new(),
    );
    let block = Block::new(header, Vec::new());
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        Some(block),
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetTime")
        .expect("time syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_int()
        .expect("int");
    let value = result.to_u64().expect("fits u64");
    assert_eq!(value, 42_000);
}

#[test]
fn runtime_log_emits_event() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut tx = Transaction::new();
    tx.set_script(vec![0x01]);
    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    engine
        .push(StackItem::from_byte_string("hello".as_bytes()))
        .expect("push message");
    engine.runtime_log().expect("runtime log");

    assert_eq!(engine.logs().len(), 1);
    assert_eq!(engine.logs()[0].message, "hello");
}

#[test]
fn runtime_log_syscall_allows_notify() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut tx = Transaction::new();
    tx.set_script(vec![0x01]);
    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push_string("hello");
    script
        .emit_syscall("System.Runtime.Log")
        .expect("log syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.logs().len(), 1);
    assert_eq!(engine.logs()[0].message, "hello");
}

#[test]
fn runtime_platform_returns_neo() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.Platform")
        .expect("platform syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result item");
    let bytes = result.as_bytes().expect("bytes");
    assert_eq!(bytes, b"NEO".to_vec());
}

#[test]
fn runtime_get_network_returns_protocol_setting() {
    let snapshot = Arc::new(DataCache::new(false));
    let settings = ProtocolSettings::default();
    let expected = settings.network as i64;
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetNetwork")
        .expect("network syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_int()
        .expect("int");
    let value = result.to_i64().expect("fits i64");
    assert_eq!(value, expected);
}

#[test]
fn runtime_get_address_version_returns_protocol_setting() {
    let snapshot = Arc::new(DataCache::new(false));
    let settings = ProtocolSettings::default();
    let expected = settings.address_version as i64;
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetAddressVersion")
        .expect("address version syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_int()
        .expect("int");
    let value = result.to_i64().expect("fits i64");
    assert_eq!(value, expected);
}

#[test]
fn runtime_get_invocation_counter_returns_one() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script
        .emit_syscall("System.Runtime.GetInvocationCounter")
        .expect("invocation counter syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_int()
        .expect("int");
    let value = result.to_i64().expect("fits i64");
    assert_eq!(value, 1);
}

#[test]
fn runtime_check_witness_returns_false_without_container() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push(&[0u8; 20]);
    script
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(!result);
}

#[test]
fn runtime_check_witness_accepts_valid_signer() {
    let key_pair = KeyPair::new(vec![1u8; 32]).expect("keypair");
    let signer_account = key_pair.get_script_hash();
    let pubkey = key_pair.compressed_public_key();

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(
        signer_account,
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_script(vec![0x01]);

    let snapshot = Arc::new(DataCache::new(false));
    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push_byte_array(&pubkey);
    script
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    script.emit_push_byte_array(&signer_account.to_bytes());
    script
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let second = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    let first = engine
        .result_stack()
        .peek(1)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(first);
    assert!(second);
}

#[test]
fn runtime_check_witness_returns_false_without_matching_signer() {
    let key_pair = KeyPair::new(vec![1u8; 32]).expect("keypair");
    let signer_account = key_pair.get_script_hash();
    let pubkey = key_pair.compressed_public_key();

    let mut tx = Transaction::new();
    tx.set_signers(Vec::new());
    tx.set_script(vec![0x01]);

    let snapshot = Arc::new(DataCache::new(false));
    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push_byte_array(&pubkey);
    script
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    script.emit_push_byte_array(&signer_account.to_bytes());
    script
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let second = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    let first = engine
        .result_stack()
        .peek(1)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(!first);
    assert!(!second);
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

fn manifest_with(
    name: &str,
    methods: Vec<ContractMethodDescriptor>,
    events: Vec<ContractEventDescriptor>,
) -> ContractManifest {
    let abi = ContractAbi::new(methods, events);
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

fn make_engine_with_sign_data() -> (ApplicationEngine, Vec<u8>) {
    let settings = ProtocolSettings::default();
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    tx.set_script(vec![0x01]);

    let hash = tx.hash();
    let mut sign_data = Vec::with_capacity(4 + UInt256::LENGTH);
    sign_data.extend_from_slice(&settings.network.to_le_bytes());
    sign_data.extend_from_slice(&hash.as_bytes());

    let container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx);
    let engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::new(DataCache::new(false)),
        None,
        settings,
        400_000_000,
        None,
    )
    .expect("engine");

    (engine, sign_data)
}

fn emit_byte_array_array(builder: &mut ScriptBuilder, items: &[Vec<u8>]) {
    for item in items {
        builder.emit_push_byte_array(item);
    }
    builder.emit_push_int(items.len() as i64);
    builder.emit_pack();
}

#[test]
fn runtime_get_calling_script_hash_returns_null_for_entry_context() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetCallingScriptHash")
        .expect("syscall");
    builder.emit_opcode(OpCode::RET);

    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result item");
    assert!(result.is_null());
}

#[test]
fn contract_create_standard_account_matches_csharp() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let pubkey = hex_decode("024b817ef37f2fc3d4a33fe36687e592d9f30fe24b3e28187dc8f12b3b3b2b839e")
        .expect("pubkey bytes");
    let account = engine
        .create_standard_account(&pubkey)
        .expect("standard account");
    assert_eq!(
        hex_encode(account.to_bytes()),
        "c44ea575c5f79638f0e73f39d7bd4b3337c81691"
    );
}

#[test]
fn runtime_get_executing_and_entry_script_hash_match_entry() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetExecutingScriptHash")
        .expect("syscall");
    builder
        .emit_syscall("System.Runtime.GetEntryScriptHash")
        .expect("syscall");
    builder.emit_opcode(OpCode::RET);

    let script = builder.to_array();
    let expected = UInt160::from_script(&script).to_bytes();

    engine
        .load_script(script, CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 2);
    let entry = engine
        .result_stack()
        .peek(0)
        .expect("entry result")
        .as_bytes()
        .expect("entry bytes");
    let executing = engine
        .result_stack()
        .peek(1)
        .expect("executing result")
        .as_bytes()
        .expect("executing bytes");

    assert_eq!(entry, expected);
    assert_eq!(executing, expected);
}

#[test]
fn runtime_get_calling_script_hash_matches_dynamic_caller() {
    let snapshot = Arc::new(DataCache::new(false));

    let mut callee_builder = ScriptBuilder::new();
    callee_builder
        .emit_syscall("System.Runtime.GetCallingScriptHash")
        .expect("syscall");
    callee_builder.emit_opcode(OpCode::RET);
    let callee_script = callee_builder.to_array();

    let method = ContractMethodDescriptor::new(
        "test".to_string(),
        Vec::new(),
        ContractParameterType::Hash160,
        0,
        false,
    )
    .expect("method");
    let callee_manifest = manifest_with("callee", vec![method], Vec::new());
    let callee_nef = NefFile::new("callee".to_string(), callee_script);
    let callee_hash =
        ContractState::calculate_hash(&UInt160::zero(), callee_nef.checksum, "callee");
    let callee_contract = ContractState::new(1, callee_hash, callee_nef, callee_manifest);
    add_contract_to_snapshot(snapshot.as_ref(), &callee_contract);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut caller_builder = ScriptBuilder::new();
    caller_builder.emit_opcode(OpCode::NEWARRAY0);
    caller_builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    caller_builder.emit_push_string("test");
    caller_builder.emit_push_byte_array(&callee_hash.to_bytes());
    caller_builder
        .emit_syscall("System.Contract.Call")
        .expect("contract call");
    caller_builder.emit_opcode(OpCode::RET);

    let caller_script = caller_builder.to_array();
    let expected = UInt160::from_script(&caller_script).to_bytes();

    engine
        .load_script(caller_script, CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine.result_stack().peek(0).expect("result item");
    let bytes = result.as_bytes().expect("calling hash bytes");
    assert_eq!(bytes, expected);
}

#[test]
fn runtime_check_witness_rejects_invalid_length() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let mut builder = ScriptBuilder::new();
    builder.emit_push(&[0x01]);
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("syscall");
    builder.emit_opcode(OpCode::RET);

    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

#[test]
fn runtime_get_notifications_reports_all_and_filtered() {
    let snapshot = Arc::new(DataCache::new(false));

    let mut callee_builder = ScriptBuilder::new();
    callee_builder.emit_push_string("testEvent2");
    callee_builder.emit_push_int(1);
    callee_builder.emit_push_int(1);
    callee_builder.emit_pack();
    callee_builder
        .emit_syscall("System.Runtime.Notify")
        .expect("notify syscall");
    callee_builder.emit_opcode(OpCode::RET);
    let callee_script = callee_builder.to_array();

    let event_param =
        ContractParameterDefinition::new("arg".to_string(), ContractParameterType::Any)
            .expect("param");
    let event =
        ContractEventDescriptor::new("testEvent2".to_string(), vec![event_param]).expect("event");
    let method = ContractMethodDescriptor::new(
        "test".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        false,
    )
    .expect("method");
    let callee_manifest = manifest_with("callee", vec![method], vec![event]);
    let callee_nef = NefFile::new("callee".to_string(), callee_script);
    let callee_hash =
        ContractState::calculate_hash(&UInt160::zero(), callee_nef.checksum, "callee");
    let callee_contract = ContractState::new(1, callee_hash, callee_nef, callee_manifest);
    add_contract_to_snapshot(snapshot.as_ref(), &callee_contract);

    let mut caller_builder = ScriptBuilder::new();
    caller_builder.emit_push_string("testEvent1");
    caller_builder.emit_opcode(OpCode::NEWARRAY0);
    caller_builder
        .emit_syscall("System.Runtime.Notify")
        .expect("notify syscall");

    caller_builder.emit_opcode(OpCode::NEWARRAY0);
    caller_builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    caller_builder.emit_push_string("test");
    caller_builder.emit_push_byte_array(&callee_hash.to_bytes());
    caller_builder
        .emit_syscall("System.Contract.Call")
        .expect("contract call");

    caller_builder.emit_opcode(OpCode::PUSHNULL);
    caller_builder
        .emit_syscall("System.Runtime.GetNotifications")
        .expect("get notifications");
    caller_builder.emit_push_byte_array(&callee_hash.to_bytes());
    caller_builder
        .emit_syscall("System.Runtime.GetNotifications")
        .expect("get notifications filtered");
    caller_builder.emit_opcode(OpCode::RET);

    let caller_script = caller_builder.to_array();
    let expected_entry_hash = UInt160::from_script(&caller_script);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    engine
        .load_script(caller_script, CallFlags::ALL, None)
        .expect("load script");
    let entry_event =
        ContractEventDescriptor::new("testEvent1".to_string(), Vec::new()).expect("event");
    let entry_manifest = manifest_with("entry", Vec::new(), vec![entry_event]);
    let entry_nef = NefFile::new("entry".to_string(), vec![OpCode::RET as u8]);
    let entry_hash = ContractState::calculate_hash(&UInt160::zero(), entry_nef.checksum, "entry");
    let entry_contract = ContractState::new(7, entry_hash, entry_nef, entry_manifest);
    let state_arc = engine.current_execution_state().expect("execution context");
    state_arc.lock().contract = Some(entry_contract);

    engine.execute().expect("execute");

    let mut all_index = None;
    let mut filtered_index = None;
    let mut max_len = 0;
    for idx in 0..engine.result_stack().len() {
        if let StackItem::Array(items) = engine.result_stack().peek(idx).expect("stack item") {
            let len = items.items().len();
            if len == 1 {
                filtered_index = Some(idx);
            }
            if len > max_len {
                max_len = len;
                all_index = Some(idx);
            }
        }
    }

    let all = engine
        .result_stack()
        .peek(all_index.expect("all notifications"))
        .expect("all notifications item");
    let filtered = engine
        .result_stack()
        .peek(filtered_index.expect("filtered notifications"))
        .expect("filtered notifications item");

    let StackItem::Array(filtered_items) = filtered else {
        panic!("expected filtered array, got {filtered:?}");
    };
    let StackItem::Array(all_items) = all else {
        panic!("expected all array, got {all:?}");
    };

    let filtered_values = filtered_items.items();
    assert_eq!(filtered_values.len(), 1);

    let mut found_event1 = false;
    let mut found_event2 = false;
    for item in all_items.items() {
        let StackItem::Array(fields) = item else {
            continue;
        };
        let field_items = fields.items();
        if field_items.len() < 2 {
            continue;
        }
        let script_hash = field_items[0].as_bytes().expect("script hash bytes");
        let event_name = field_items[1].as_bytes().expect("event name bytes");
        if script_hash == expected_entry_hash.to_bytes() && event_name == b"testEvent1" {
            found_event1 = true;
        }
        if script_hash == callee_hash.to_bytes() && event_name == b"testEvent2" {
            found_event2 = true;
        }
    }
    assert!(found_event1, "missing entry notification");
    assert!(found_event2, "missing callee notification");

    let filtered_event = &filtered_values[0];
    let StackItem::Array(filtered_fields) = filtered_event else {
        panic!("expected filtered notification array, got {filtered_event:?}");
    };
    let filtered_field_items = filtered_fields.items();
    let script_hash = filtered_field_items[0]
        .as_bytes()
        .expect("script hash bytes");
    assert_eq!(script_hash, callee_hash.to_bytes());
}

#[test]
fn crypto_checksig_accepts_valid_signature() {
    let (mut engine, sign_data) = make_engine_with_sign_data();

    let private_key = [0x01u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("pubkey");
    let signature = Secp256r1Crypto::sign(&sign_data, &private_key).expect("signature");

    let mut script = ScriptBuilder::new();
    script.emit_push_byte_array(&signature);
    script.emit_push_byte_array(&public_key);
    script
        .emit_syscall("System.Crypto.CheckSig")
        .expect("checksig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(result);
}

#[test]
fn crypto_checksig_faults_on_invalid_pubkey_length() {
    let (mut engine, _sign_data) = make_engine_with_sign_data();

    let invalid_pubkey = vec![0x02u8; 70];
    let signature = vec![0x01u8; 64];

    let mut script = ScriptBuilder::new();
    script.emit_push_byte_array(&signature);
    script.emit_push_byte_array(&invalid_pubkey);
    script
        .emit_syscall("System.Crypto.CheckSig")
        .expect("checksig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

#[test]
fn crypto_checkmultisig_accepts_valid_signatures() {
    let (mut engine, sign_data) = make_engine_with_sign_data();

    let private_key1 = [0x01u8; 32];
    let private_key2 = [0x02u8; 32];
    let pubkey1 = Secp256r1Crypto::derive_public_key(&private_key1).expect("pubkey1");
    let pubkey2 = Secp256r1Crypto::derive_public_key(&private_key2).expect("pubkey2");
    let sig1 = Secp256r1Crypto::sign(&sign_data, &private_key1).expect("sig1");
    let sig2 = Secp256r1Crypto::sign(&sign_data, &private_key2).expect("sig2");

    let mut script = ScriptBuilder::new();
    emit_byte_array_array(&mut script, &[sig1.to_vec(), sig2.to_vec()]);
    emit_byte_array_array(&mut script, &[pubkey1, pubkey2]);
    script
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("checkmultisig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(result);
}

#[test]
fn crypto_checkmultisig_faults_on_empty_pubkeys() {
    let (mut engine, sign_data) = make_engine_with_sign_data();

    let private_key = [0x01u8; 32];
    let signature = Secp256r1Crypto::sign(&sign_data, &private_key).expect("signature");

    let mut script = ScriptBuilder::new();
    emit_byte_array_array(&mut script, &[signature.to_vec()]);
    script.emit_opcode(OpCode::NEWARRAY0);
    script
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("checkmultisig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

#[test]
fn crypto_checkmultisig_returns_false_on_invalid_signature() {
    let (mut engine, sign_data) = make_engine_with_sign_data();

    let private_key1 = [0x01u8; 32];
    let private_key2 = [0x02u8; 32];
    let pubkey1 = Secp256r1Crypto::derive_public_key(&private_key1).expect("pubkey1");
    let pubkey2 = Secp256r1Crypto::derive_public_key(&private_key2).expect("pubkey2");
    let sig1 = Secp256r1Crypto::sign(&sign_data, &private_key1).expect("sig1");
    let invalid_sig = vec![0u8; 64];

    let mut script = ScriptBuilder::new();
    emit_byte_array_array(&mut script, &[sig1.to_vec(), invalid_sig]);
    emit_byte_array_array(&mut script, &[pubkey1, pubkey2]);
    script
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("checkmultisig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let result = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bool()
        .expect("bool");
    assert!(!result);
}

#[test]
fn crypto_checkmultisig_faults_on_invalid_pubkey_length() {
    let (mut engine, sign_data) = make_engine_with_sign_data();

    let private_key1 = [0x01u8; 32];
    let private_key2 = [0x02u8; 32];
    let pubkey1 = Secp256r1Crypto::derive_public_key(&private_key1).expect("pubkey1");
    let invalid_pubkey = vec![0x02u8; 70];
    let sig1 = Secp256r1Crypto::sign(&sign_data, &private_key1).expect("sig1");
    let sig2 = Secp256r1Crypto::sign(&sign_data, &private_key2).expect("sig2");

    let mut script = ScriptBuilder::new();
    emit_byte_array_array(&mut script, &[sig1.to_vec(), sig2.to_vec()]);
    emit_byte_array_array(&mut script, &[pubkey1, invalid_pubkey]);
    script
        .emit_syscall("System.Crypto.CheckMultisig")
        .expect("checkmultisig syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

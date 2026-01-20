use neo_core::hardfork::Hardfork;
use neo_core::ledger::{Block, BlockHeader};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractManifest, ContractParameterDefinition,
    ContractPermission, WildCardContainer,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::witness::Witness;
use neo_core::{IVerifiable, UInt160, WitnessScope};
use neo_vm::stack_item::{Array, Pointer};
use neo_vm::{OpCode, Script, ScriptBuilder, StackItem, StackItemType};
use num_bigint::BigInt;
use std::any::Any;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug)]
struct DummyInterop;

impl neo_vm::stack_item::InteropInterface for DummyInterop {
    fn interface_type(&self) -> &str {
        "DummyInterop"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn make_test_transaction(sender: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(0);
    tx.set_script(vec![OpCode::PUSH2 as u8]);
    tx.set_attributes(Vec::new());
    tx.set_signers(vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn make_persisting_block(nonce: u64) -> Block {
    let mut header = BlockHeader::default();
    header.nonce = nonce;
    Block::new(header, Vec::new())
}

fn build_get_random_script(count: usize) -> Vec<u8> {
    let mut script = ScriptBuilder::new();
    for _ in 0..count {
        script
            .emit_syscall("System.Runtime.GetRandom")
            .expect("getrandom syscall");
    }
    script.emit_opcode(OpCode::RET);
    script.to_array()
}

fn run_get_random(engine: &mut ApplicationEngine, count: usize) -> Vec<BigInt> {
    let script = build_get_random_script(count);
    engine
        .load_script(script, CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    let len = engine.result_stack().len();
    assert_eq!(len, count);
    let mut values = Vec::with_capacity(len);
    for index in (0..len).rev() {
        let value = engine
            .result_stack()
            .peek(index)
            .expect("result item")
            .as_int()
            .expect("int")
            .clone();
        values.push(value);
    }
    values
}

fn install_notify_contract(engine: &mut ApplicationEngine, param_type: ContractParameterType) {
    let param = ContractParameterDefinition::new("arg".to_string(), param_type).expect("param");
    let event = ContractEventDescriptor::new("e1".to_string(), vec![param]).expect("event");
    let abi = ContractAbi::new(Vec::new(), vec![event]);
    let manifest = ContractManifest {
        name: "notify".to_string(),
        groups: Vec::new(),
        features: Default::default(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::create_wildcard(),
        extra: None,
    };
    let nef = NefFile::new("notify".to_string(), vec![OpCode::RET as u8]);
    let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, "notify");
    let contract = ContractState::new(1, hash, nef, manifest);

    let state = engine.current_execution_state().expect("execution state");
    state.lock().contract = Some(contract);
}

fn push_notify_args(engine: &mut ApplicationEngine, event_name: &str, state: StackItem) {
    engine
        .push(StackItem::from_byte_string(event_name.as_bytes()))
        .expect("push name");
    engine.push(state).expect("push state");
}

fn big(value: &str) -> BigInt {
    BigInt::from_str(value).expect("bigint")
}

#[test]
fn runtime_get_random_same_block_matches_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfAspidochelone, 0);
    let expected = vec![
        big("271339657438512451304577787170704246350"),
        big("98548189559099075644778613728143131367"),
        big("247654688993873392544380234598471205121"),
        big("291082758879475329976578097236212073607"),
        big("247152297361212656635216876565962360375"),
    ];

    let tx = make_test_transaction(UInt160::zero());
    let container: Arc<dyn IVerifiable> = Arc::new(tx);
    let mut engine_1 = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::clone(&container)),
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings.clone(),
        1100_00000000,
        None,
    )
    .expect("engine");
    let mut engine_2 = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");

    let rand_1 = run_get_random(&mut engine_1, expected.len());
    let rand_2 = run_get_random(&mut engine_2, expected.len());

    assert_eq!(rand_1, expected);
    assert_eq!(rand_2, expected);
}

#[test]
fn runtime_get_random_different_transactions_diverge() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfAspidochelone, 0);
    let tx_1 = make_test_transaction(UInt160::zero());
    let mut tx_2 = Transaction::new();
    tx_2.set_nonce(2_083_236_893);
    tx_2.set_signers(Vec::new());
    tx_2.set_attributes(Vec::new());
    tx_2.set_script(Vec::new());
    tx_2.set_witnesses(Vec::new());

    let mut engine_1 = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx_1)),
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings.clone(),
        1100_00000000,
        None,
    )
    .expect("engine");
    let mut engine_2 = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx_2)),
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");

    let rand_1 = run_get_random(&mut engine_1, 5);
    let rand_2 = run_get_random(&mut engine_2, 5);

    for (left, right) in rand_1.iter().zip(rand_2.iter()) {
        assert_ne!(left, right);
    }
}

#[test]
fn runtime_log_rejects_invalid_utf8() {
    let settings = ProtocolSettings::default();
    let tx = make_test_transaction(UInt160::zero());
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx)),
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::NONE, None)
        .expect("load script");

    let msg = vec![
        68, 216, 160, 6, 89, 102, 86, 72, 37, 15, 132, 45, 76, 221, 170, 21, 128, 51, 34, 168, 205,
        56, 10, 228, 51, 114, 4, 218, 245, 155, 172, 132,
    ];
    engine
        .push(StackItem::from_byte_string(msg))
        .expect("push log bytes");
    let err = engine.runtime_log().expect_err("invalid utf8 should fail");
    assert!(err.contains("Invalid UTF-8 sequence"));
}

#[test]
fn runtime_notify_rejects_circular_state() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfBasilisk, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    install_notify_contract(&mut engine, ContractParameterType::Array);

    let array = Array::new_untracked(Vec::new());
    let item = StackItem::Array(array.clone());
    array.push(item.clone()).expect("push");

    let err = engine
        .ensure_notification_size(&[item])
        .expect_err("circular notify should fail");
    assert!(err.contains("Circular reference"));
}

#[test]
fn runtime_notify_coerces_buffer_to_byte_string() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfBasilisk, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    install_notify_contract(&mut engine, ContractParameterType::ByteArray);

    let buffer = StackItem::from_buffer(vec![0u8]);
    let state = StackItem::from_array(vec![buffer]);
    push_notify_args(&mut engine, "e1", state);

    let start_len = engine.notifications().len();
    engine.runtime_notify().expect("notify succeeds");
    assert_eq!(engine.notifications().len(), start_len + 1);
    assert_eq!(
        engine.notifications()[start_len].state[0].stack_item_type(),
        StackItemType::ByteString
    );
}

#[test]
fn runtime_notify_rejects_pointer_argument() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfBasilisk, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    install_notify_contract(&mut engine, ContractParameterType::ByteArray);

    let script = Arc::new(Script::new_relaxed(Vec::new()));
    let pointer = Pointer::new(script, 1);
    let state = StackItem::from_array(vec![StackItem::Pointer(pointer)]);
    push_notify_args(&mut engine, "e1", state);

    let err = engine.runtime_notify().expect_err("pointer should fail");
    assert!(err.contains("does not match"));
}

#[test]
fn runtime_notify_rejects_interop_interface_argument() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfBasilisk, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(make_persisting_block(2_083_236_893)),
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load script");
    install_notify_contract(&mut engine, ContractParameterType::InteropInterface);

    let interop = StackItem::InteropInterface(Arc::new(DummyInterop));
    let state = StackItem::from_array(vec![interop]);
    push_notify_args(&mut engine, "e1", state);

    let err = engine.runtime_notify().expect_err("interop should fail");
    assert!(err.contains("Unsupported stack item type"));
}

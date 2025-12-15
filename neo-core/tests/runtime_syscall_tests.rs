use neo_core::network::p2p::payloads::{Signer, Transaction, WitnessScope};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::UInt160;
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
    assert_eq!(value, 7);
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
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.state(), VMState::HALT);
    assert_eq!(engine.result_stack().len(), 1);

    let result = engine.result_stack().peek(0).expect("result item");
    let StackItem::Array(signers) = result else {
        panic!("expected Array result, got {result:?}");
    };

    assert_eq!(signers.items().len(), 1);
    let signer_item = &signers.items()[0];

    let StackItem::Array(fields) = signer_item else {
        panic!("expected Signer array, got {signer_item:?}");
    };

    assert_eq!(fields.items().len(), 5);
    let encoded_account = fields.items()[0].as_bytes().expect("account bytes");
    assert_eq!(encoded_account, account.to_bytes());

    let scopes = fields.items()[1]
        .as_int()
        .expect("scopes int")
        .to_u8()
        .expect("scopes fits u8");
    assert_eq!(scopes, WitnessScope::GLOBAL.bits());

    for index in 2..5 {
        let StackItem::Array(array) = &fields.items()[index] else {
            panic!("expected array at {index}, got {:?}", fields.items()[index]);
        };
        assert!(array.items().is_empty());
    }
}

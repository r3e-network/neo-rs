use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_vm::vm_state::VMState;
use neo_vm::{OpCode, ScriptBuilder};
use std::sync::Arc;

#[test]
fn runtime_notify_requires_allow_notify_flag() {
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
    builder.emit_push_string("evt");
    builder.emit_push_int(0);
    builder.emit_pack();
    builder
        .emit_syscall("System.Runtime.Notify")
        .expect("notify syscall");

    builder.emit_opcode(OpCode::RET);

    engine
        .load_script(builder.to_array(), CallFlags::READ_STATES, None)
        .expect("load script");

    assert!(engine.execute().is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

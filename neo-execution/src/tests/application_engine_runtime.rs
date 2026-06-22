use super::*;
use crate::application_engine::TEST_MODE_GAS;
use neo_config::ProtocolSettings;
use neo_primitives::TriggerType;
use neo_storage::DataCache;
use neo_vm::Script;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use std::sync::Arc;

#[test]
fn notification_parameter_type_check_matches_csharp() {
    assert!(!matches_parameter_type(
        &StackItem::null(),
        ContractParameterType::String
    ));
    assert!(matches_parameter_type(
        &StackItem::from_byte_string(b"neo".to_vec()),
        ContractParameterType::String
    ));
    assert!(!matches_parameter_type(
        &StackItem::from_byte_string(vec![0xff]),
        ContractParameterType::String
    ));

    let pointer = StackItem::from_pointer(Arc::new(Script::new_from_bytes(vec![])), 0);
    for expected in [
        ContractParameterType::Any,
        ContractParameterType::ByteArray,
        ContractParameterType::InteropInterface,
    ] {
        assert!(
            !matches_parameter_type(&pointer, expected),
            "C# CheckItemType rejects Pointer before matching {expected:?}"
        );
    }
}

#[test]
fn runtime_log_allows_dynamic_script_without_container_like_csharp() {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_string("dynamic log");
    builder
        .emit_syscall("System.Runtime.Log")
        .expect("emit Runtime.Log");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);

    let log = engine.logs().first().expect("log event");
    assert!(log.script_container.is_none());
    assert_eq!(log.message, "dynamic log");
}

#[test]
fn send_notification_enforces_echidna_cap_for_native_paths_like_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    for _ in 0..crate::application_engine::MAX_NOTIFICATION_COUNT {
        engine
            .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
            .expect("notification below cap");
    }

    let err = engine
        .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
        .expect_err("513th application notification must fault after Echidna");
    assert!(err.to_string().contains("Maximum number of notifications"));
    assert_eq!(
        engine.notifications().len(),
        crate::application_engine::MAX_NOTIFICATION_COUNT
    );
}

#[test]
fn get_notifications_deep_copies_domovoi_state_like_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfDomovoi, 0);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    let nested_arg = StackItem::from_array(vec![StackItem::from_i64(1)]);
    engine
        .send_notification(UInt160::zero(), "Native".to_string(), vec![nested_arg])
        .expect("send notification");

    let StackItem::Array(stored_nested) = &engine.notifications()[0].state[0] else {
        panic!("stored notification argument should be an array");
    };
    assert!(stored_nested.is_read_only());
    assert!(stored_nested.push(StackItem::from_i64(2)).is_err());

    let notifications = engine.get_notifications(None).expect("get notifications");
    let StackItem::Array(notification) = &notifications[0] else {
        panic!("notification should project as an array");
    };
    let fields = notification.items();
    assert_eq!(fields.len(), 3);

    let StackItem::Array(state) = &fields[2] else {
        panic!("notification state should project as an array");
    };
    assert!(state.is_read_only());

    let state_items = state.items();
    let StackItem::Array(returned_nested) = &state_items[0] else {
        panic!("returned notification argument should be an array");
    };
    assert!(returned_nested.is_read_only());
    assert_ne!(returned_nested.id(), stored_nested.id());
    assert!(returned_nested.push(StackItem::from_i64(3)).is_err());
}

#[test]
fn notification_size_check_uses_stack_value_serializer() {
    let source = include_str!("../application_engine_helper.rs");
    let start = source
        .find("pub fn ensure_notification_size")
        .expect("ensure_notification_size exists");
    let end = source[start..]
        .find("pub fn send_notification")
        .map(|offset| start + offset)
        .expect("send_notification follows size check");
    let helper = &source[start..end];

    assert!(helper.contains("notification_state_to_stack_value"));
    assert!(helper.contains("serialize_stack_value_with_limits"));
    assert!(!helper.contains("StackItem::from_array(state.to_vec())"));
    assert!(!helper.contains("BinarySerializer::serialize(&StackItem"));
}

#[test]
fn get_notifications_non_domovoi_projection_uses_stack_value_adapter() {
    let source = include_str!("../application_engine_helper.rs");
    let start = source
        .find("fn notification_to_stack_item")
        .expect("notification projection helper exists");
    let end = source[start..]
        .find("fn notification_state_to_stack_value")
        .map(|offset| start + offset)
        .expect("stack value helper follows notification projection");
    let helper = &source[start..end];

    assert!(helper.contains("readonly_array_stack_item"));
    assert!(helper.contains("notification_state_to_stack_value(&notification.state)"));
    assert!(helper.contains("StackItem::try_from(value)"));
    assert!(!helper.contains("StackItem::from_array(notification.state.to_vec())"));
}

#[test]
fn runtime_check_witness_faults_on_invalid_public_key_like_csharp() {
    let invalid_public_key = [0x05; 33];
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&invalid_public_key);
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("emit Runtime.CheckWitness");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::FAULT);
}

#[test]
fn verification_trigger_without_persisting_block_uses_configured_hardforks_like_csharp() {
    let settings = ProtocolSettings::default();
    assert!(settings.hardforks.contains_key(&Hardfork::HfAspidochelone));

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetRandom")
        .expect("emit Runtime.GetRandom");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(engine.fee_consumed(), (1 << 13) * 30);
}

#[test]
fn invocation_counter_uses_explicit_context_script_hash_like_csharp() {
    let logical_hash = UInt160::from_bytes(&[0x42; 20]).expect("logical hash");

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetInvocationCounter")
        .expect("emit Runtime.GetInvocationCounter");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, Some(logical_hash))
        .expect("load script with logical hash");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);

    let result = engine
        .result_stack()
        .peek(0)
        .expect("invocation counter result")
        .as_int()
        .expect("integer result");
    assert_eq!(result, 1.into());
}

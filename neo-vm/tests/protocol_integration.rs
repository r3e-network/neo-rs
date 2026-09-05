use neo_vm::{
    BinarySerializer, ExecutionEngine, ExecutionEngineLimits, JumpTable, OpCode, Script, StackItem,
    VmState,
};

#[test]
fn public_vm_api_executes_script_and_exposes_halt_state() {
    let mut engine = ExecutionEngine::new(Some(JumpTable::default()));
    engine
        .load_script(
            Script::new_relaxed(vec![
                OpCode::PUSH1.byte(),
                OpCode::PUSH2.byte(),
                OpCode::ADD.byte(),
                OpCode::RET.byte(),
            ]),
            -1,
            0,
        )
        .expect("load script");

    assert_eq!(engine.execute(), VmState::HALT);
    assert_eq!(
        engine.result_stack().peek(0).expect("result"),
        &StackItem::from_int(3)
    );
}

#[test]
fn public_serializer_roundtrips_stack_value_with_default_limits() {
    let value = neo_vm::StackValue::Array(vec![
        neo_vm::StackValue::Boolean(true),
        neo_vm::StackValue::BigInteger(vec![0x80]),
    ]);
    let bytes = BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
        .expect("serialize");
    let decoded = BinarySerializer::deserialize_stack_value(&bytes).expect("deserialize");
    assert_eq!(decoded, value);
}

#[test]
fn item_size_limit_matches_canonical_neo_vm_value() {
    let limits = ExecutionEngineLimits::default();
    assert_eq!(limits.max_item_size, 131_070);
    assert_eq!(limits.max_comparable_size, 65_536);
    assert!(limits.assert_max_item_size(131_070).is_ok());
    assert!(limits.assert_max_item_size(131_071).is_err());
}

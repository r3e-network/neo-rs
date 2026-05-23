use neo_core::neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_core::smart_contract::native::hash_index_state::HashIndexState;
use neo_core::smart_contract::BinarySerializer;
use neo_core::UInt256;
use neo_vm_rs::StackValue;

#[test]
fn hash_index_state_round_trips_via_stack_value() {
    let origin = HashIndexState::new(UInt256::zero(), 42);
    let limits = ExecutionEngineLimits::default();

    let stack_representation = origin.to_stack_value();
    assert!(matches!(stack_representation, StackValue::Struct(_)));

    let serialized =
        BinarySerializer::serialize_stack_value(&stack_representation, &limits).expect("serialize");
    let deserialized = BinarySerializer::deserialize_stack_value(&serialized).expect("deserialize");

    let mut dest = HashIndexState::default();
    dest.from_stack_value(deserialized).unwrap();

    assert_eq!(origin.hash, dest.hash);
    assert_eq!(origin.index, dest.index);
}

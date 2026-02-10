use neo_core::smart_contract::native::hash_index_state::HashIndexState;
use neo_core::smart_contract::BinarySerializer;
use neo_core::smart_contract::IInteroperable;
use neo_core::UInt256;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;

#[test]
fn hash_index_state_round_trips_via_stack_item() {
    let origin = HashIndexState::new(UInt256::zero(), 42);
    let limits = ExecutionEngineLimits::default();

    let stack_representation = origin.to_stack_item().unwrap();
    assert!(matches!(stack_representation, neo_vm::StackItem::Struct(_)));

    let serialized =
        BinarySerializer::serialize(&stack_representation, &limits).expect("serialize");
    let deserialized =
        BinarySerializer::deserialize(&serialized, &limits, None).expect("deserialize");

    let mut dest = HashIndexState::default();
    dest.from_stack_item(deserialized).unwrap();

    assert_eq!(origin.hash, dest.hash);
    assert_eq!(origin.index, dest.index);
}

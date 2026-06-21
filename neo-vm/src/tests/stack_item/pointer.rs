use super::*;

#[test]
fn pointer_equality_depends_on_script_identity() {
    let script_a = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));
    let script_b = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));

    let ptr_a1 = Pointer::new(Arc::clone(&script_a), 10);
    let ptr_a2 = Pointer::new(Arc::clone(&script_a), 10);
    let ptr_b = Pointer::new(script_b, 10);

    assert_eq!(ptr_a1, ptr_a2);
    assert_ne!(ptr_a1, ptr_b);
}

#[test]
fn pointer_ordering_uses_script_identity_then_position() {
    let script = Arc::new(Script::new_relaxed(vec![0x01]));
    let ptr_1 = Pointer::new(Arc::clone(&script), 1);
    let ptr_2 = Pointer::new(Arc::clone(&script), 2);

    assert!(ptr_1 < ptr_2);
}

#[test]
fn pointer_to_boolean_and_integer() {
    let script = Arc::new(Script::new_relaxed(vec![0x01]));
    let pointer = Pointer::new(script, 42);

    assert!(pointer.to_boolean());
    assert_eq!(pointer.to_integer(), BigInt::from(42));
    assert_eq!(pointer.stack_item_type(), StackItemType::Pointer);
}

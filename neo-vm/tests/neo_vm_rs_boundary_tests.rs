use neo_vm::collections::VmOrderedDictionary;
use neo_vm::{Script, StackItem, VMState};
use neo_vm_rs::{StackValue, VmState};
use num_bigint::BigInt;
use std::any::TypeId;
use std::sync::Arc;

#[test]
fn public_boundary_types_are_sourced_from_neo_vm_rs() {
    assert_eq!(
        TypeId::of::<neo_vm::StackValue>(),
        TypeId::of::<neo_vm_rs::StackValue>()
    );
    assert_eq!(
        TypeId::of::<neo_vm::VmState>(),
        TypeId::of::<neo_vm_rs::VmState>()
    );
    assert_eq!(
        TypeId::of::<neo_vm::ExecutionResult>(),
        TypeId::of::<neo_vm_rs::ExecutionResult>()
    );
}

#[test]
fn vm_state_converts_to_and_from_neo_vm_rs_final_states() {
    assert_eq!(VMState::from(VmState::Halt), VMState::HALT);
    assert_eq!(VMState::from(VmState::Fault), VMState::FAULT);

    assert_eq!(VmState::try_from(VMState::HALT).unwrap(), VmState::Halt);
    assert_eq!(VmState::try_from(VMState::FAULT).unwrap(), VmState::Fault);
    assert!(VmState::try_from(VMState::NONE).is_err());
    assert!(VmState::try_from(VMState::BREAK).is_err());
}

#[test]
fn primitive_stack_items_convert_to_neo_vm_rs_stack_values() {
    assert_eq!(
        StackValue::try_from(StackItem::from_i64(42)).unwrap(),
        StackValue::Integer(42)
    );
    assert_eq!(
        StackValue::try_from(StackItem::from_int(BigInt::from(i64::MAX) + 1)).unwrap(),
        StackValue::BigInteger(vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00])
    );
    assert_eq!(
        StackValue::try_from(StackItem::from_byte_string([1, 2, 3])).unwrap(),
        StackValue::ByteString(vec![1, 2, 3])
    );
    assert_eq!(
        StackValue::try_from(StackItem::from_buffer([4, 5])).unwrap(),
        StackValue::Buffer(vec![4, 5])
    );
    assert_eq!(
        StackValue::try_from(StackItem::from_bool(true)).unwrap(),
        StackValue::Boolean(true)
    );
    assert_eq!(
        StackValue::try_from(StackItem::null()).unwrap(),
        StackValue::Null
    );
}

#[test]
fn primitive_neo_vm_rs_stack_values_convert_to_stack_items() {
    assert_eq!(
        StackItem::try_from(StackValue::Integer(-42))
            .unwrap()
            .as_int()
            .unwrap(),
        BigInt::from(-42)
    );
    assert_eq!(
        StackItem::try_from(StackValue::BigInteger(vec![0xff]))
            .unwrap()
            .as_int()
            .unwrap(),
        BigInt::from(-1)
    );
    assert_eq!(
        StackItem::try_from(StackValue::ByteString(vec![1, 2, 3]))
            .unwrap()
            .as_bytes()
            .unwrap(),
        vec![1, 2, 3]
    );
    assert_eq!(
        StackItem::try_from(StackValue::Buffer(vec![4, 5]))
            .unwrap()
            .as_bytes()
            .unwrap(),
        vec![4, 5]
    );
    assert_eq!(
        StackItem::try_from(StackValue::Boolean(true))
            .unwrap()
            .as_bool()
            .unwrap(),
        true
    );
    assert!(matches!(
        StackItem::try_from(StackValue::Null).unwrap(),
        StackItem::Null
    ));
}

#[test]
fn compound_stack_items_convert_to_and_from_neo_vm_rs_stack_values() {
    let array = StackItem::from_array(vec![
        StackItem::from_bool(true),
        StackItem::from_byte_string([0xaa]),
    ]);
    assert_eq!(
        StackValue::try_from(array).unwrap(),
        StackValue::Array(vec![
            StackValue::Boolean(true),
            StackValue::ByteString(vec![0xaa])
        ])
    );

    let structure = StackItem::try_from(StackValue::Struct(vec![
        StackValue::Integer(7),
        StackValue::Null,
    ]))
    .unwrap();
    assert!(matches!(structure, StackItem::Struct(_)));
    assert_eq!(structure.as_array().unwrap().len(), 2);

    let mut entries = VmOrderedDictionary::new();
    entries.insert(
        StackItem::from_byte_string([0x01]),
        StackItem::from_int(9),
    );
    let map = StackItem::from_map(entries);
    assert_eq!(
        StackValue::try_from(map).unwrap(),
        StackValue::Map(vec![(
            StackValue::ByteString(vec![0x01]),
            StackValue::Integer(9)
        )])
    );

    let map = StackItem::try_from(StackValue::Map(vec![(
        StackValue::ByteString(vec![0x02]),
        StackValue::Boolean(false),
    )]))
    .unwrap();
    assert!(matches!(map, StackItem::Map(_)));
    assert_eq!(map.as_map().unwrap().len(), 1);
}

#[test]
fn runtime_identity_stack_items_do_not_claim_lossless_inbound_conversion() {
    let script = Arc::new(Script::new_relaxed(vec![0x11]));
    let pointer = StackItem::from_pointer(script, 3);
    assert_eq!(
        StackValue::try_from(pointer).unwrap(),
        StackValue::Pointer(3)
    );

    assert!(StackItem::try_from(StackValue::Pointer(3)).is_err());
    assert!(StackItem::try_from(StackValue::Interop(7)).is_err());
    assert!(StackItem::try_from(StackValue::Iterator(7)).is_err());
}

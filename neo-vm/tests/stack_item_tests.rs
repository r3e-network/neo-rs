#![allow(clippy::mutable_key_type)]
//! Integration tests for the Neo VM stack items.

use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::script::Script;
use neo_vm::stack_item::stack_item_type::StackItemType;
use neo_vm::stack_item::StackItem;
use num_bigint::BigInt;
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn test_boolean_stack_item() {
    let true_item = StackItem::from_bool(true);
    let false_item = StackItem::from_bool(false);

    assert!(true_item.as_bool().unwrap());
    assert!(!false_item.as_bool().unwrap());
    assert_eq!(true_item.stack_item_type(), StackItemType::Boolean);

    // Test conversion to integer
    assert_eq!(true_item.as_int().unwrap(), BigInt::from(1));
    assert_eq!(false_item.as_int().unwrap(), BigInt::from(0));

    // Test conversion to bytes
    assert_eq!(true_item.as_bytes().unwrap(), vec![1]);
    assert_eq!(false_item.as_bytes().unwrap(), vec![0]);

    // Test equality
    assert!(true_item.equals(&true_item).unwrap());
    assert!(false_item.equals(&false_item).unwrap());
    assert!(!true_item.equals(&false_item).unwrap());
}

#[test]
fn test_integer_stack_item() {
    let int_item = StackItem::from_int(42);
    let negative_item = StackItem::from_int(-42);
    let zero_item = StackItem::from_int(0);

    assert_eq!(int_item.as_int().unwrap(), BigInt::from(42));
    assert_eq!(negative_item.as_int().unwrap(), BigInt::from(-42));
    assert_eq!(zero_item.as_int().unwrap(), BigInt::from(0));

    assert_eq!(int_item.stack_item_type(), StackItemType::Integer);

    // Test conversion to boolean
    assert!(int_item.as_bool().unwrap());
    assert!(negative_item.as_bool().unwrap());
    assert!(!zero_item.as_bool().unwrap());

    // Test conversion to bytes
    assert_eq!(int_item.as_bytes().unwrap(), vec![42]);
    assert_eq!(zero_item.as_bytes().unwrap(), Vec::<u8>::new());

    // Encoding matches .NET two's complement semantics
    let high_bit = StackItem::from_int(128);
    assert_eq!(high_bit.as_bytes().unwrap(), vec![0x80, 0x00]);
    let neg_one = StackItem::from_int(-1);
    assert_eq!(neg_one.as_bytes().unwrap(), vec![0xFF]);
    let neg_128 = StackItem::from_int(-128);
    assert_eq!(neg_128.as_bytes().unwrap(), vec![0x80]);

    // Test equality
    assert!(int_item.equals(&int_item).unwrap());
    assert!(!int_item.equals(&negative_item).unwrap());
    assert!(!int_item.equals(&zero_item).unwrap());
}

#[test]
fn test_byte_string_stack_item() {
    let bytes = vec![1, 2, 3];
    let byte_string = StackItem::from_byte_string(bytes.clone());
    let empty_bytes = StackItem::from_byte_string(Vec::<u8>::new());

    assert_eq!(byte_string.as_bytes().unwrap(), bytes);
    assert_eq!(empty_bytes.as_bytes().unwrap(), Vec::<u8>::new());

    assert_eq!(byte_string.stack_item_type(), StackItemType::ByteString);

    // Test conversion to boolean
    assert!(byte_string.as_bool().unwrap());
    assert!(!empty_bytes.as_bool().unwrap());

    // Test conversion to integer
    assert_eq!(byte_string.as_int().unwrap(), BigInt::from(0x030201));
    assert_eq!(empty_bytes.as_int().unwrap(), BigInt::from(0));

    // Test equality
    assert!(byte_string.equals(&byte_string).unwrap());
    assert!(!byte_string.equals(&empty_bytes).unwrap());
}

#[test]
fn test_buffer_stack_item() {
    let bytes = vec![1, 2, 3];
    let buffer = StackItem::from_buffer(bytes.clone());
    let empty_buffer = StackItem::from_buffer(Vec::<u8>::new());

    assert_eq!(buffer.as_bytes().unwrap(), bytes);
    assert_eq!(empty_buffer.as_bytes().unwrap(), Vec::<u8>::new());

    assert_eq!(buffer.stack_item_type(), StackItemType::Buffer);

    // Test conversion to boolean
    assert!(buffer.as_bool().unwrap());
    assert!(empty_buffer.as_bool().unwrap());

    // Test conversion to integer
    assert_eq!(buffer.as_int().unwrap(), BigInt::from(0x030201));
    assert_eq!(empty_buffer.as_int().unwrap(), BigInt::from(0));

    // Test equality
    assert!(buffer.equals(&buffer).unwrap());
    assert!(!buffer.equals(&empty_buffer).unwrap());

    // Test equality with ByteString
    let byte_string = StackItem::from_byte_string(bytes.clone());
    assert!(buffer.equals(&byte_string).unwrap());
}

#[test]
fn test_array_stack_item() {
    let array = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);
    let empty_array = StackItem::from_array(Vec::<StackItem>::new());

    assert_eq!(array.as_array().unwrap().len(), 3);
    assert_eq!(empty_array.as_array().unwrap().len(), 0);

    assert_eq!(array.stack_item_type(), StackItemType::Array);

    // Test conversion to boolean
    assert!(array.as_bool().unwrap());
    assert!(empty_array.as_bool().unwrap());

    // Test equality
    assert!(array.equals(&array).unwrap());
    assert!(!array.equals(&empty_array).unwrap());

    // Test deep equality
    let array2 = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);
    assert!(array.equals(&array2).unwrap());

    let array3 = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(4),
    ]);
    assert!(!array.equals(&array3).unwrap());
}

#[test]
fn test_struct_stack_item() {
    let struct_item = StackItem::from_struct(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);
    let empty_struct = StackItem::from_struct(Vec::<StackItem>::new());

    assert_eq!(struct_item.as_array().unwrap().len(), 3);
    assert_eq!(empty_struct.as_array().unwrap().len(), 0);

    assert_eq!(struct_item.stack_item_type(), StackItemType::Struct);

    // Test conversion to boolean
    assert!(struct_item.as_bool().unwrap());
    assert!(empty_struct.as_bool().unwrap());

    // Test equality
    assert!(struct_item.equals(&struct_item).unwrap());
    assert!(!struct_item.equals(&empty_struct).unwrap());

    // Test deep equality
    let struct_item2 = StackItem::from_struct(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);
    assert!(struct_item.equals(&struct_item2).unwrap());
    assert!(struct_item
        .equals_with_limits(&struct_item2, &ExecutionEngineLimits::default())
        .unwrap());

    let struct_item3 = StackItem::from_struct(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(4),
    ]);
    assert!(!struct_item.equals(&struct_item3).unwrap());
}

#[test]
fn test_map_stack_item() {
    let mut map = BTreeMap::new();
    map.insert(StackItem::from_int(1), StackItem::from_int(10));
    map.insert(StackItem::from_int(2), StackItem::from_int(20));
    map.insert(StackItem::from_int(3), StackItem::from_int(30));

    let map_item = StackItem::from_map(map.clone());
    let empty_map = StackItem::from_map(BTreeMap::new());

    assert_eq!(map_item.as_map().unwrap().len(), 3);
    assert_eq!(empty_map.as_map().unwrap().len(), 0);

    assert_eq!(map_item.stack_item_type(), StackItemType::Map);

    // Test conversion to boolean
    assert!(map_item.as_bool().unwrap());
    assert!(empty_map.as_bool().unwrap());

    // Test equality
    assert!(map_item.equals(&map_item).unwrap());
    assert!(!map_item.equals(&empty_map).unwrap());

    // Test deep equality
    let mut map2 = BTreeMap::new();
    map2.insert(StackItem::from_int(1), StackItem::from_int(10));
    map2.insert(StackItem::from_int(2), StackItem::from_int(20));
    map2.insert(StackItem::from_int(3), StackItem::from_int(30));

    let map_item2 = StackItem::from_map(map2);
    assert!(map_item.equals(&map_item2).unwrap());

    let mut map3 = BTreeMap::new();
    map3.insert(StackItem::from_int(1), StackItem::from_int(10));
    map3.insert(StackItem::from_int(2), StackItem::from_int(20));
    map3.insert(StackItem::from_int(3), StackItem::from_int(40));

    let map_item3 = StackItem::from_map(map3);
    assert!(!map_item.equals(&map_item3).unwrap());
}

#[test]
fn test_pointer_stack_item() {
    let script = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));
    let pointer = StackItem::from_pointer(Arc::clone(&script), 42);
    let zero_pointer = StackItem::from_pointer(script, 0);

    assert_eq!(pointer.stack_item_type(), StackItemType::Pointer);

    // Test conversion to boolean
    assert!(pointer.as_bool().unwrap());
    assert!(zero_pointer.as_bool().unwrap());

    // Test equality
    assert!(pointer.equals(&pointer).unwrap());
    assert!(!pointer.equals(&zero_pointer).unwrap());
}

#[test]
fn test_null_stack_item() {
    let null = StackItem::null();

    assert_eq!(null.stack_item_type(), StackItemType::Any);

    // Test conversion to boolean
    assert!(!null.as_bool().unwrap());

    // Test equality
    assert!(null.equals(&null).unwrap());
    assert!(!null.equals(&StackItem::from_int(0)).unwrap());
}

#[test]
fn test_deep_clone() {
    // Test deep clone of complex nested structure
    let array = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_array(vec![
            StackItem::from_int(3),
            StackItem::from_int(4),
            StackItem::from_struct(vec![StackItem::from_int(5), StackItem::from_int(6)]),
        ]),
    ]);

    let original_len = array.as_array().unwrap().len();
    let mut cloned = array.deep_clone();
    assert!(array.equals(&cloned).unwrap());

    // Ensure it's a deep copy
    let array_ref = array.as_array().unwrap();
    let nested_array = &array_ref[2];
    let nested_array_ref = nested_array.as_array().unwrap();
    let nested_struct = &nested_array_ref[2];
    let nested_struct_ref = nested_struct.as_array().unwrap();

    assert_eq!(nested_struct_ref.len(), 2);
    assert_eq!(nested_struct_ref[0].as_int().unwrap(), BigInt::from(5));
    assert_eq!(nested_struct_ref[1].as_int().unwrap(), BigInt::from(6));

    if let StackItem::Array(ref mut cloned_items) = cloned {
        cloned_items.push(StackItem::from_int(99)).unwrap();
    }
    let cloned_ref = cloned.as_array().unwrap();
    let cloned_nested_array = &cloned_ref[2];
    let cloned_nested_array_ref = cloned_nested_array.as_array().unwrap();
    let cloned_nested_struct = &cloned_nested_array_ref[2];
    let cloned_nested_struct_ref = cloned_nested_struct.as_array().unwrap();

    assert_eq!(cloned_nested_struct_ref.len(), 2);
    assert_eq!(
        cloned_nested_struct_ref[0].as_int().unwrap(),
        BigInt::from(5)
    );
    assert_eq!(
        cloned_nested_struct_ref[1].as_int().unwrap(),
        BigInt::from(6)
    );

    // Deep clone should not mutate the original when modified
    assert_eq!(array.as_array().unwrap().len(), original_len);
    assert_eq!(cloned.as_array().unwrap().len(), original_len + 1);
    assert!(!array.equals(&cloned).unwrap());
}

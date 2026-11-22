use super::*;

#[test]
fn test_boolean_stack_item() {
    let true_item = StackItem::from_bool(true);
    let false_item = StackItem::from_bool(false);

    assert!(true_item.as_bool().expect("Failed to convert to bool"));
    assert!(!false_item.as_bool().expect("Failed to convert to bool"));
    assert_eq!(true_item.stack_item_type(), StackItemType::Boolean);
}

#[test]
fn test_integer_stack_item() {
    let int_item = StackItem::from_int(42);

    assert_eq!(int_item.as_int().unwrap(), BigInt::from(42));
    assert!(int_item.as_bool().expect("Failed to convert"));
    assert_eq!(int_item.stack_item_type(), StackItemType::Integer);

    let zero_item = StackItem::from_int(0);
    assert!(!zero_item.as_bool().expect("Failed to convert"));
}

#[test]
fn test_bytestring_stack_item() {
    let bytes = vec![1, 2, 3];
    let byte_string = StackItem::from_byte_string(bytes.clone());

    assert_eq!(byte_string.as_bytes().unwrap(), bytes);
    assert!(byte_string.as_bool().expect("Failed to convert"));
    assert_eq!(byte_string.stack_item_type(), StackItemType::ByteString);

    let empty_bytes = StackItem::from_byte_string(Vec::<u8>::new());
    assert!(!empty_bytes.as_bool().expect("Failed to convert"));
}

#[test]
fn test_array_stack_item() {
    let array = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);

    assert_eq!(
        array
            .as_array()
            .expect("intermediate value should exist")
            .len(),
        3
    );
    assert!(array.as_bool().expect("Failed to convert"));
    assert_eq!(array.stack_item_type(), StackItemType::Array);

    let empty_array = StackItem::from_array(Vec::<StackItem>::new());
    assert!(empty_array.as_bool().expect("Failed to convert"));
}

#[test]
fn test_deep_clone() {
    let array = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_array(vec![StackItem::from_int(3), StackItem::from_int(4)]),
    ]);

    let cloned = array.deep_clone();
    assert!(array.equals(&cloned).unwrap());
}

#[test]
fn test_equals() {
    let a = StackItem::from_int(42);
    let b = StackItem::from_int(42);
    let c = StackItem::from_int(43);

    assert!(a.equals(&b).unwrap());
    assert!(!a.equals(&c).unwrap());

    let array1 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

    let array2 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

    let array3 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(3)]);

    assert!(array1.equals(&array2).unwrap_or(false));
    assert!(!array1.equals(&array3).unwrap());
}

#[test]
fn test_convert_to() {
    let int_item = StackItem::from_int(42);

    // Convert to boolean
    let bool_item = int_item.convert_to(StackItemType::Boolean).unwrap();
    assert_eq!(bool_item.stack_item_type(), StackItemType::Boolean);
    assert!(bool_item.as_bool().expect("Failed to convert"));

    let byte_string = int_item.convert_to(StackItemType::ByteString).unwrap();
    assert_eq!(byte_string.stack_item_type(), StackItemType::ByteString);
    assert_eq!(byte_string.as_bytes().unwrap(), vec![42]);

    // Convert to buffer
    let buffer = int_item.convert_to(StackItemType::Buffer).unwrap();
    assert_eq!(buffer.stack_item_type(), StackItemType::Buffer);
    assert_eq!(buffer.as_bytes().unwrap(), vec![42]);

    let int_clone = int_item.convert_to(StackItemType::Integer).unwrap();
    assert_eq!(int_clone.stack_item_type(), StackItemType::Integer);
    assert_eq!(
        int_clone.as_int().expect("Operation failed"),
        BigInt::from(42)
    );

    // Convert to unsupported type
    assert!(int_item.convert_to(StackItemType::Array).is_err());
}

#[test]
fn convert_to_boolean_preserves_local_truthiness_boundaries() {
    fn assert_boolean(item: StackItem, expected: bool) {
        let converted = item.convert_to(StackItemType::Boolean).unwrap();
        assert_eq!(converted, StackItem::Boolean(expected));
    }

    assert_eq!(
        StackItem::Null.convert_to(StackItemType::Boolean).unwrap(),
        StackItem::Null
    );
    assert_boolean(StackItem::from_int(0), false);
    assert_boolean(StackItem::from_int(1), true);
    assert_boolean(StackItem::from_byte_string(vec![0]), false);
    assert_boolean(StackItem::from_byte_string(vec![1]), true);

    assert_boolean(StackItem::from_buffer(Vec::<u8>::new()), true);
    assert_boolean(StackItem::from_buffer(vec![0]), true);
    assert_boolean(StackItem::from_array(Vec::<StackItem>::new()), true);
    assert_boolean(StackItem::from_struct(Vec::<StackItem>::new()), true);

    let too_large = StackItem::from_byte_string(vec![0; VM_INTEGER_MAX_SIZE + 1]);
    assert!(too_large.convert_to(StackItemType::Boolean).is_err());
}

#[test]
fn test_equals_with_cycles() {
    // Create two arrays with cycles
    let mut array1 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

    let mut array2 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

    // Add cycles
    let array1_clone = array1.clone();
    if let StackItem::Array(array) = &mut array1 {
        let _ = array.push(array1_clone);
    }

    let array2_clone = array2.clone();
    if let StackItem::Array(array) = &mut array2 {
        let _ = array.push(array2_clone);
    }

    // The arrays should be equal despite the cycles
    assert!(array1.equals(&array2).unwrap_or(false));
}

#[test]
fn array_and_struct_ordering_compare_length_then_items() {
    let short_array = StackItem::from_array(vec![StackItem::from_int(1)]);
    let long_array = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
    assert_eq!(short_array.cmp(&long_array), std::cmp::Ordering::Less);

    let lower_array = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
    let higher_array = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(3)]);
    assert_eq!(lower_array.cmp(&higher_array), std::cmp::Ordering::Less);

    let short_struct = StackItem::from_struct(vec![StackItem::from_int(1)]);
    let long_struct = StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(2)]);
    assert_eq!(short_struct.cmp(&long_struct), std::cmp::Ordering::Less);

    let lower_struct = StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(2)]);
    let higher_struct =
        StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(3)]);
    assert_eq!(lower_struct.cmp(&higher_struct), std::cmp::Ordering::Less);
}

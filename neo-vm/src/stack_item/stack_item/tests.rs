use super::*;

use neo_vm_rs::{ExecutionEngineLimits, StackItemType, VmOrderedDictionary};
use num_bigint::BigInt;

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
fn stack_value_big_integer_conversion_enforces_vm_integer_max_size() {
    let negative = StackItem::try_from(neo_vm_rs::StackValue::BigInteger(vec![0xff]))
        .expect("valid BigInteger bytes should convert");
    assert_eq!(negative.as_int().unwrap(), BigInt::from(-1));

    let too_large = neo_vm_rs::StackValue::BigInteger(vec![0x01; VM_INTEGER_MAX_SIZE + 1]);
    assert!(StackItem::try_from(too_large).is_err());
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

#[test]
fn equal_opcode_faults_on_oversized_byte_strings() {
    // C# ByteString.Equals(other, ref limits) throws when Size > MaxComparableSize
    // (ByteString.cs:62). MaxComparableSize default is 65536.
    let limits = ExecutionEngineLimits::default();
    let big = limits.max_comparable_size as usize + 1;
    let a = StackItem::from_byte_string(vec![0u8; big]);
    let b = StackItem::from_byte_string(vec![0u8; big]);

    assert!(
        a.equals_with_limits(&b, &limits).is_err(),
        "EQUAL on two >MaxComparableSize ByteStrings must fault"
    );
    // The same items still compare structurally via the un-limited PartialEq path.
    assert!(a.equals(&b).unwrap());
}

#[test]
fn equal_opcode_faults_on_oversized_struct_item_count() {
    // C# Struct.Equals throws "Too many struct items" once more than MaxStackSize
    // items are popped during the walk (Struct.cs:102-103).
    let limits = ExecutionEngineLimits::default();
    let count = limits.max_stack_size as usize + 64;

    let a_inner: Vec<StackItem> = (0..count).map(|_| StackItem::from_int(1)).collect();
    let b_inner: Vec<StackItem> = (0..count).map(|_| StackItem::from_int(1)).collect();
    let a = StackItem::from_struct(a_inner);
    let b = StackItem::from_struct(b_inner);

    assert!(
        a.equals_with_limits(&b, &limits).is_err(),
        "EQUAL on a Struct with >MaxStackSize items must fault"
    );
}

#[test]
fn equal_opcode_uses_reference_semantics_for_arrays() {
    // C# Array does NOT override Equals, so under EQUAL it falls back to
    // ReferenceEquals (StackItem.cs:117-120): structurally-identical but distinct
    // arrays compare NOT equal.
    let limits = ExecutionEngineLimits::default();
    let a = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
    let b = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

    assert!(
        !a.equals_with_limits(&b, &limits).unwrap(),
        "distinct equal-valued arrays must be NOT equal under EQUAL (reference semantics)"
    );
    // Same reference compares equal.
    assert!(a.equals_with_limits(&a.clone(), &limits).unwrap());
    // The structural PartialEq path is unchanged and still reports equal.
    assert!(a.equals(&b).unwrap());
}

#[test]
fn equal_opcode_uses_reference_semantics_for_maps() {
    // C# Map likewise falls back to ReferenceEquals under EQUAL.
    let limits = ExecutionEngineLimits::default();
    let mut d1 = VmOrderedDictionary::new();
    d1.insert(StackItem::from_int(1), StackItem::from_int(2));
    let mut d2 = VmOrderedDictionary::new();
    d2.insert(StackItem::from_int(1), StackItem::from_int(2));
    let a = StackItem::from_map(d1);
    let b = StackItem::from_map(d2);

    assert!(
        !a.equals_with_limits(&b, &limits).unwrap(),
        "distinct equal-valued maps must be NOT equal under EQUAL (reference semantics)"
    );
    assert!(a.equals_with_limits(&a.clone(), &limits).unwrap());
}

#[test]
fn equal_opcode_preserves_primitive_type_strictness() {
    // Verified against mainnet C# v3.9.1: Integer(1) != ByteString([1]) under EQUAL.
    let limits = ExecutionEngineLimits::default();
    let int_one = StackItem::from_int(1);
    let bytes_one = StackItem::from_byte_string(vec![1u8]);

    assert!(!int_one.equals_with_limits(&bytes_one, &limits).unwrap());
    // Dispatch is on self's type: ByteString.Equals(Integer) is also false.
    assert!(!bytes_one.equals_with_limits(&int_one, &limits).unwrap());
    // Same-type value equality still holds.
    assert!(
        int_one
            .equals_with_limits(&StackItem::from_int(1), &limits)
            .unwrap()
    );
    assert!(
        bytes_one
            .equals_with_limits(&StackItem::from_byte_string(vec![1u8]), &limits)
            .unwrap()
    );
}

#[test]
fn equal_opcode_struct_byte_strings_share_comparable_budget() {
    // Inside Struct.Equals the byte-string budget is threaded across all nested
    // ByteString comparisons (Struct.cs:106-108). A struct whose nested byte
    // strings sum to more than MaxComparableSize must fault.
    let limits = ExecutionEngineLimits::default();
    let chunk = (limits.max_comparable_size / 2) as usize + 1;
    let a = StackItem::from_struct(vec![
        StackItem::from_byte_string(vec![0u8; chunk]),
        StackItem::from_byte_string(vec![0u8; chunk]),
    ]);
    let b = StackItem::from_struct(vec![
        StackItem::from_byte_string(vec![0u8; chunk]),
        StackItem::from_byte_string(vec![0u8; chunk]),
    ]);

    assert!(
        a.equals_with_limits(&b, &limits).is_err(),
        "nested byte strings exceeding the shared comparable budget must fault"
    );
}

mod buffer_bytestring_equal_tests {
    use super::*;
    use crate::stack_item::buffer::Buffer as BufferItem;

    #[test]
    fn buffer_never_equals_bytestring() {
        // In C# Neo VM, Buffer uses reference equality (compound type).
        // Buffer == ByteString is always false, even with same content.
        let bs = StackItem::ByteString(vec![0x01]);
        let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
        assert!(
            !bs.equals(&buf).unwrap(),
            "ByteString(01) should NOT equal Buffer(01)"
        );
        assert!(
            !buf.equals(&bs).unwrap(),
            "Buffer(01) should NOT equal ByteString(01)"
        );
    }

    #[test]
    fn buffer_reference_equality() {
        // Same Buffer instance equals itself
        let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
        assert!(buf.equals(&buf).unwrap(), "Buffer should equal itself");
    }
}

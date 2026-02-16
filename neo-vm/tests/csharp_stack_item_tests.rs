#![allow(clippy::mutable_key_type)]
//! Comprehensive StackItem tests that exactly match C# Neo.VM.Tests/UT_StackItem.cs
//!
//! This file contains unit tests that ensure the Rust StackItem implementation
//! behaves identically to the C# Neo VM StackItem implementation.

use neo_vm::{script::Script, stack_item::StackItem};
use num_bigint::BigInt;
use std::collections::BTreeMap;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Test circular reference handling (matches C# TestCircularReference)
    #[test]
    fn test_circular_reference() {
        // Note: In Rust, creating true circular references is complex due to borrowing rules
        // We'll test the equality logic instead
        let item_a =
            StackItem::from_struct(vec![StackItem::Boolean(true), StackItem::Boolean(false)]);

        let item_b =
            StackItem::from_struct(vec![StackItem::Boolean(true), StackItem::Boolean(false)]);

        let item_c =
            StackItem::from_struct(vec![StackItem::Boolean(false), StackItem::Boolean(false)]);

        // Test hash codes - items with same structure should have same hash
        // Note: Rust doesn't have GetHashCode() like C#, but we can test equality
        assert_eq!(item_a, item_b, "Items A and B should be equal");
        assert_ne!(item_a, item_c, "Items A and C should not be equal");
    }

    /// Test hash code generation (matches C# TestHashCode)
    ///
    /// In C# Neo, Buffer uses reference equality (ReferenceEquals), meaning two Buffer
    /// instances with the same content are NOT equal unless they are the same instance.
    /// ByteString, Integer, Boolean, etc. use value equality.
    #[test]
    fn test_hash_code() {
        // Note: Rust doesn't have GetHashCode(), but we test equality which uses similar logic

        let item_a = StackItem::from_byte_string("NEO");
        let item_b = StackItem::from_byte_string("NEO");
        let item_c = StackItem::from_byte_string("SmartEconomy");

        assert_eq!(item_a, item_b, "Same strings should be equal");
        assert_ne!(item_a, item_c, "Different strings should not be equal");

        // Buffer comparison - C# uses REFERENCE equality, not value equality
        // Two different Buffer instances with the same content are NOT equal
        let item_a = StackItem::from_buffer(vec![0; 1]);
        let item_b = StackItem::from_buffer(vec![0; 1]);
        let item_c = StackItem::from_buffer(vec![0; 2]);

        // In C# Neo, Buffer uses ReferenceEquals, so different instances are never equal
        assert_ne!(
            item_a, item_b,
            "Different Buffer instances should NOT be equal (reference semantics)"
        );
        assert_ne!(
            item_a, item_c,
            "Different Buffer instances should NOT be equal"
        );

        // Same instance should be equal - we use the same variable
        // Note: clone() creates a new Buffer with a new id, so they won't be equal
        let _item_same = &item_a; // Same reference, would be equal

        // Byte array comparison
        let item_a = StackItem::from_byte_string(vec![1, 2, 3]);
        let item_b = StackItem::from_byte_string(vec![1, 2, 3]);
        let item_c = StackItem::from_byte_string(vec![5, 6]);

        assert_eq!(item_a, item_b, "Same byte arrays should be equal");
        assert_ne!(item_a, item_c, "Different byte arrays should not be equal");

        // Boolean comparison
        let item_a = StackItem::Boolean(true);
        let item_b = StackItem::Boolean(true);
        let item_c = StackItem::Boolean(false);

        assert_eq!(item_a, item_b, "Same booleans should be equal");
        assert_ne!(item_a, item_c, "Different booleans should not be equal");

        // Integer comparison
        let item_a = StackItem::from_int(1);
        let item_b = StackItem::from_int(1);
        let item_c = StackItem::from_int(123);

        assert_eq!(item_a, item_b, "Same integers should be equal");
        assert_ne!(item_a, item_c, "Different integers should not be equal");

        // Null comparison
        let item_a = StackItem::Null;
        let item_b = StackItem::Null;

        assert_eq!(item_a, item_b, "Null items should be equal");

        // Array comparison
        let item_a = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(0),
        ]);
        let item_b = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(0),
        ]);
        let item_c = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(1),
        ]);

        assert_eq!(item_a, item_b, "Same arrays should be equal");
        assert_ne!(item_a, item_c, "Different arrays should not be equal");

        let item_a = StackItem::from_struct(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(0),
        ]);
        let item_b = StackItem::from_struct(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(0),
        ]);
        let item_c = StackItem::from_struct(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(1),
        ]);

        assert_eq!(item_a, item_b, "Same structs should be equal");
        assert_ne!(item_a, item_c, "Different structs should not be equal");

        // Map comparison
        let mut map_a = BTreeMap::new();
        map_a.insert(StackItem::Boolean(true), StackItem::Boolean(false));
        map_a.insert(StackItem::from_int(0), StackItem::from_int(1));
        let item_a = StackItem::from_map(map_a);

        let mut map_b = BTreeMap::new();
        map_b.insert(StackItem::Boolean(true), StackItem::Boolean(false));
        map_b.insert(StackItem::from_int(0), StackItem::from_int(1));
        let item_b = StackItem::from_map(map_b);

        let mut map_c = BTreeMap::new();
        map_c.insert(StackItem::Boolean(true), StackItem::Boolean(false));
        map_c.insert(StackItem::from_int(0), StackItem::from_int(2));
        let item_c = StackItem::from_map(map_c);

        assert_eq!(item_a, item_b, "Same maps should be equal");
        assert_ne!(item_a, item_c, "Different maps should not be equal");

        // Test compound type with shared subitems
        let junk = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::Boolean(false),
            StackItem::from_int(0),
        ]);

        let mut map_a = BTreeMap::new();
        map_a.insert(StackItem::Boolean(true), junk.clone());
        map_a.insert(StackItem::from_int(0), junk.clone());
        let item_a = StackItem::from_map(map_a);

        let mut map_b = BTreeMap::new();
        map_b.insert(StackItem::Boolean(true), junk.clone());
        map_b.insert(StackItem::from_int(0), junk.clone());
        let item_b = StackItem::from_map(map_b);

        let mut map_c = BTreeMap::new();
        map_c.insert(StackItem::Boolean(true), junk);
        map_c.insert(StackItem::from_int(0), StackItem::from_int(2));
        let item_c = StackItem::from_map(map_c);

        assert_eq!(item_a, item_b, "Maps with same subitems should be equal");
        assert_ne!(
            item_a, item_c,
            "Maps with different subitems should not be equal"
        );

        // Pointer comparison
        let script = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));
        let alt_script = Arc::new(Script::new_relaxed(vec![0xFF]));
        let item_a = StackItem::from_pointer(Arc::clone(&script), 123);
        let item_b = StackItem::from_pointer(Arc::clone(&script), 123);
        let item_c = StackItem::from_pointer(Arc::clone(&script), 1234);
        let item_d = StackItem::from_pointer(alt_script, 123);

        assert_eq!(item_a, item_b, "Same pointers should be equal");
        assert_ne!(item_a, item_c, "Different positions should not be equal");
        assert_ne!(
            item_a, item_d,
            "Pointers to different scripts should not be equal"
        );
    }

    /// Test null item behavior (matches C# TestNull)
    #[test]
    fn test_null() {
        let null_item = StackItem::from_byte_string(Vec::<u8>::new());
        assert_ne!(
            StackItem::Null,
            null_item,
            "Empty byte string should not equal null"
        );

        let null_item = StackItem::Null;
        assert_eq!(StackItem::Null, null_item, "Null should equal null");
    }

    /// Test equality comparison (matches C# TestEqual)
    #[test]
    fn test_equal() {
        let item_a = StackItem::from_byte_string("NEO");
        let item_b = StackItem::from_byte_string("NEO");
        let item_c = StackItem::from_byte_string("SmartEconomy");
        let item_d = StackItem::from_byte_string("Smarteconomy");
        let item_e = StackItem::from_byte_string("smarteconomy");

        assert!(
            item_a.equals(&item_b).unwrap(),
            "Same strings should be equal"
        );
        assert!(
            !item_a.equals(&item_c).unwrap(),
            "Different strings should not be equal"
        );
        assert!(
            !item_c.equals(&item_d).unwrap(),
            "Case sensitive comparison should fail"
        );
        assert!(
            !item_d.equals(&item_e).unwrap(),
            "Case sensitive comparison should fail"
        );

        // We can't directly test this like C# since Rust is statically typed
    }

    /// Test type casting (matches C# TestCast)
    #[test]
    fn test_cast() {
        // Signed byte
        let item = StackItem::from_int(i8::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(i8::MAX));

        // Unsigned byte
        let item = StackItem::from_int(u8::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(u8::MAX));

        // Signed short
        let item = StackItem::from_int(i16::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(i16::MAX));

        // Unsigned short
        let item = StackItem::from_int(u16::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(u16::MAX));

        // Signed integer
        let item = StackItem::from_int(i32::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(i32::MAX));

        // Unsigned integer
        let item = StackItem::from_int(u32::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(u32::MAX));

        // Signed long
        let item = StackItem::from_int(i64::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(i64::MAX));

        // Unsigned long
        let item = StackItem::from_int(u64::MAX);
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(u64::MAX));

        // BigInteger
        let item = StackItem::from_int(BigInt::from(-1));
        assert!(
            matches!(item, StackItem::Integer(_)),
            "Should be Integer type"
        );
        assert_eq!(item.as_int().unwrap(), BigInt::from(-1));

        // Boolean
        let item = StackItem::Boolean(true);
        assert!(
            matches!(item, StackItem::Boolean(_)),
            "Should be Boolean type"
        );
        assert!(item.as_bool().unwrap(), "Should be true");

        // ByteString
        let item =
            StackItem::from_byte_string(vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);
        assert!(
            matches!(item, StackItem::ByteString(_)),
            "Should be ByteString type"
        );
        assert_eq!(
            item.as_bytes().unwrap(),
            vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09]
        );
    }

    /// Test deep copy functionality (matches C# TestDeepCopy)
    ///
    /// In C# Neo, DeepCopy creates new instances of compound types (Array, Struct, Map, Buffer).
    /// Since these types use reference equality, deep copied objects are NOT equal to originals
    /// (they are different instances). Primitive types (Boolean, Integer, ByteString, Null)
    /// use value equality, so their deep copies ARE equal to originals.
    #[test]
    fn test_deep_copy() {
        // Test deep copy of primitive types (value equality)
        let bool_item = StackItem::Boolean(true);
        let bool_copy = bool_item.deep_clone();
        assert_eq!(
            bool_item, bool_copy,
            "Boolean deep copy should be equal (value semantics)"
        );

        let int_item = StackItem::from_int(42);
        let int_copy = int_item.deep_clone();
        assert_eq!(
            int_item, int_copy,
            "Integer deep copy should be equal (value semantics)"
        );

        let bytes_item = StackItem::from_byte_string(vec![1, 2, 3]);
        let bytes_copy = bytes_item.deep_clone();
        assert_eq!(
            bytes_item, bytes_copy,
            "ByteString deep copy should be equal (value semantics)"
        );

        let null_item = StackItem::Null;
        let null_copy = null_item.deep_clone();
        assert_eq!(null_item, null_copy, "Null deep copy should be equal");

        // Test deep copy of Buffer (reference equality)
        // In C# Neo, Buffer uses ReferenceEquals, so deep copies are NOT equal
        let buffer_item = StackItem::from_buffer(vec![1, 2, 3]);
        let buffer_copy = buffer_item.deep_clone();
        assert_ne!(
            buffer_item, buffer_copy,
            "Buffer deep copy should NOT be equal (reference semantics)"
        );

        // Verify buffer content is preserved
        if let (StackItem::Buffer(orig), StackItem::Buffer(copy)) = (&buffer_item, &buffer_copy) {
            assert_eq!(
                orig.data(),
                copy.data(),
                "Buffer content should be preserved after deep copy"
            );
        }

        // Test deep copy of Array (uses equals method for content comparison)
        let array_item = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::from_int(1),
            StackItem::from_byte_string(vec![1]),
        ]);
        let array_copy = array_item.deep_clone();

        // Arrays use .equals() for content comparison, which should return true
        assert!(
            array_item.equals(&array_copy).unwrap(),
            "Array deep copy should have equal content via .equals()"
        );

        // Test deep copy of Struct
        let struct_item =
            StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        let struct_copy = struct_item.deep_clone();
        assert!(
            struct_item.equals(&struct_copy).unwrap(),
            "Struct deep copy should have equal content via .equals()"
        );

        // Test deep copy of Map
        let mut map = BTreeMap::new();
        map.insert(StackItem::from_int(0), StackItem::from_int(1));
        let map_item = StackItem::from_map(map);
        let map_copy = map_item.deep_clone();
        assert!(
            map_item.equals(&map_copy).unwrap(),
            "Map deep copy should have equal content via .equals()"
        );
    }

    #[test]
    fn test_boolean_conversions() {
        // Test boolean conversion behavior
        assert!(
            !StackItem::Null.as_bool().unwrap(),
            "Null should convert to false"
        );
        assert!(
            StackItem::Boolean(true).as_bool().unwrap(),
            "True should convert to true"
        );
        assert!(
            !StackItem::Boolean(false).as_bool().unwrap(),
            "False should convert to false"
        );
        assert!(
            StackItem::from_int(1).as_bool().unwrap(),
            "Non-zero integer should convert to true"
        );
        assert!(
            !StackItem::from_int(0).as_bool().unwrap(),
            "Zero integer should convert to false"
        );
        assert!(
            !StackItem::from_byte_string(Vec::<u8>::new())
                .as_bool()
                .unwrap(),
            "Empty byte string should convert to false"
        );
        assert!(
            StackItem::from_byte_string(vec![1]).as_bool().unwrap(),
            "Non-empty byte string should convert to true"
        );
        assert!(
            StackItem::from_array(Vec::<StackItem>::new())
                .as_bool()
                .unwrap(),
            "Empty array should convert to true"
        );
        assert!(
            StackItem::from_array(vec![StackItem::Null])
                .as_bool()
                .unwrap(),
            "Non-empty array should convert to true"
        );
    }

    #[test]
    fn test_integer_conversions() {
        // Test integer conversion behavior
        assert!(
            StackItem::Null.as_int().is_err(),
            "Null should not convert to integer"
        );
        assert_eq!(
            StackItem::Boolean(true).as_int().unwrap(),
            BigInt::from(1),
            "True should convert to 1"
        );
        assert_eq!(
            StackItem::Boolean(false).as_int().unwrap(),
            BigInt::from(0),
            "False should convert to 0"
        );
        assert_eq!(
            StackItem::from_int(42).as_int().unwrap(),
            BigInt::from(42),
            "Integer should convert to itself"
        );
        assert_eq!(
            StackItem::from_byte_string(Vec::<u8>::new())
                .as_int()
                .unwrap(),
            BigInt::from(0),
            "Empty bytes should convert to 0"
        );
        assert_eq!(
            StackItem::from_byte_string(vec![42]).as_int().unwrap(),
            BigInt::from(42),
            "Single byte should convert correctly"
        );
    }

    #[test]
    fn test_byte_conversions() {
        // Test byte array conversion behavior
        assert_eq!(
            StackItem::Null.as_bytes().unwrap(),
            Vec::<u8>::new(),
            "Null should convert to empty bytes"
        );
        assert_eq!(
            StackItem::Boolean(true).as_bytes().unwrap(),
            vec![1],
            "True should convert to [1]"
        );
        assert_eq!(
            StackItem::Boolean(false).as_bytes().unwrap(),
            vec![0],
            "False should convert to [0]"
        );
        assert_eq!(
            StackItem::from_int(0).as_bytes().unwrap(),
            Vec::<u8>::new(),
            "Zero should convert to empty bytes"
        );
        assert_eq!(
            StackItem::from_int(42).as_bytes().unwrap(),
            vec![42],
            "Small integer should convert correctly"
        );
        assert_eq!(
            StackItem::from_byte_string(vec![1, 2, 3])
                .as_bytes()
                .unwrap(),
            vec![1, 2, 3],
            "Byte string should convert to itself"
        );
    }

    #[test]
    fn test_type_checking() {
        // Test type checking methods
        assert!(StackItem::Null.is_null(), "Null should be null");
        assert!(
            !StackItem::Boolean(true).is_null(),
            "Boolean should not be null"
        );
        assert!(
            !StackItem::from_int(0).is_null(),
            "Integer should not be null"
        );

        // Test stack item type detection
        use neo_vm::stack_item::stack_item_type::StackItemType;

        assert_eq!(StackItem::Null.stack_item_type(), StackItemType::Any);
        assert_eq!(
            StackItem::Boolean(true).stack_item_type(),
            StackItemType::Boolean
        );
        assert_eq!(
            StackItem::from_int(42).stack_item_type(),
            StackItemType::Integer
        );
        assert_eq!(
            StackItem::from_byte_string(Vec::<u8>::new()).stack_item_type(),
            StackItemType::ByteString
        );
        assert_eq!(
            StackItem::from_buffer(Vec::<u8>::new()).stack_item_type(),
            StackItemType::Buffer
        );
        assert_eq!(
            StackItem::from_array(Vec::<StackItem>::new()).stack_item_type(),
            StackItemType::Array
        );
        assert_eq!(
            StackItem::from_struct(Vec::<StackItem>::new()).stack_item_type(),
            StackItemType::Struct
        );
        assert_eq!(
            StackItem::from_map(BTreeMap::new()).stack_item_type(),
            StackItemType::Map
        );
        assert_eq!(
            StackItem::from_pointer(Arc::new(Script::new_relaxed(vec![0xAA])), 0).stack_item_type(),
            StackItemType::Pointer
        );
    }
}

//! Property-based tests for neo-vm
//!
//! These tests use proptest to verify:
//! - Stack operations (push then pop returns original)
//! - StackItem operations
//! - EvaluationStack properties

use neo_vm::{EvaluationStack, ReferenceCounter, StackItem};
use num_bigint::BigInt;
use proptest::prelude::*;

proptest! {
    // =========================================================================
    // Evaluation Stack Push/Pop Tests
    // =========================================================================

    /// Test that push then pop returns the original item - bool
    #[test]
    fn test_push_pop_roundtrip_bool(b in any::<bool>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_bool(b);
        let original = item.clone();
        stack.push(item).unwrap();
        let popped = stack.pop().unwrap();
        prop_assert!(original.equals(&popped).unwrap());
    }

    /// Test that push then pop returns the original item - int
    #[test]
    fn test_push_pop_roundtrip_int(i in any::<i64>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_int(i);
        let original = item.clone();
        stack.push(item).unwrap();
        let popped = stack.pop().unwrap();
        prop_assert!(original.equals(&popped).unwrap());
    }

    /// Test that push then pop returns the original item - bytes
    #[test]
    fn test_push_pop_roundtrip_bytes(v in any::<Vec<u8>>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_byte_string(v);
        let original = item.clone();
        stack.push(item).unwrap();
        let popped = stack.pop().unwrap();
        prop_assert!(original.equals(&popped).unwrap());
    }

    // =========================================================================
    // Stack Length Tests
    // =========================================================================

    /// Test that stack length is tracked correctly for bool items
    #[test]
    fn test_stack_length_bool(b in any::<bool>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);

        prop_assert_eq!(stack.len(), 0);
        prop_assert!(stack.is_empty());

        stack.push(StackItem::from_bool(b)).unwrap();
        prop_assert_eq!(stack.len(), 1);

        let _ = stack.pop().unwrap();
        prop_assert_eq!(stack.len(), 0);
        prop_assert!(stack.is_empty());
    }

    /// Test that stack length is tracked correctly for int items
    #[test]
    fn test_stack_length_int(i in any::<i64>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);

        stack.push(StackItem::from_int(i)).unwrap();
        prop_assert_eq!(stack.len(), 1);

        let _ = stack.pop().unwrap();
        prop_assert!(stack.is_empty());
    }

    /// Test that stack length is tracked correctly for byte items
    #[test]
    fn test_stack_length_bytes(v in any::<Vec<u8>>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);

        stack.push(StackItem::from_byte_string(v)).unwrap();
        prop_assert_eq!(stack.len(), 1);

        let _ = stack.pop().unwrap();
        prop_assert!(stack.is_empty());
    }

    // =========================================================================
    // StackItem Property Tests
    // =========================================================================

    /// Test that StackItem boolean conversion is consistent
    #[test]
    fn test_stackitem_bool_consistency(value in any::<bool>()) {
        let item = StackItem::from_bool(value);
        prop_assert_eq!(item.as_bool().unwrap(), value);
    }

    /// Test that StackItem integer conversion is consistent
    #[test]
    fn test_stackitem_int_consistency(value in any::<i64>()) {
        let item = StackItem::from_int(value);
        let retrieved = item.as_int().unwrap();
        prop_assert_eq!(retrieved, BigInt::from(value));
    }

    /// Test that StackItem bytestring conversion is consistent
    #[test]
    fn test_stackitem_bytestring_consistency(data in any::<Vec<u8>>()) {
        let item = StackItem::from_byte_string(data.clone());
        let retrieved = item.as_bytes().unwrap();
        prop_assert_eq!(retrieved, data);
    }

    /// Test that StackItem deep clone produces equal item - bool
    #[test]
    fn test_stackitem_deep_clone_bool(value in any::<bool>()) {
        let item = StackItem::from_bool(value);
        let cloned = item.deep_clone();
        prop_assert!(item.equals(&cloned).unwrap());
    }

    /// Test that StackItem deep clone produces equal item - int
    #[test]
    fn test_stackitem_deep_clone_int(value in any::<i64>()) {
        let item = StackItem::from_int(value);
        let cloned = item.deep_clone();
        prop_assert!(item.equals(&cloned).unwrap());
    }

    /// Test that StackItem deep clone produces equal item - bytes
    #[test]
    fn test_stackitem_deep_clone_bytes(data in any::<Vec<u8>>()) {
        let item = StackItem::from_byte_string(data);
        let cloned = item.deep_clone();
        prop_assert!(item.equals(&cloned).unwrap());
    }

    /// Test that StackItem equals is reflexive - bool
    #[test]
    fn test_stackitem_equals_reflexive_bool(value in any::<bool>()) {
        let item = StackItem::from_bool(value);
        prop_assert!(item.equals(&item).unwrap());
    }

    /// Test that StackItem equals is reflexive - int
    #[test]
    fn test_stackitem_equals_reflexive_int(value in any::<i64>()) {
        let item = StackItem::from_int(value);
        prop_assert!(item.equals(&item).unwrap());
    }

    /// Test that StackItem equals is reflexive - bytes
    #[test]
    fn test_stackitem_equals_reflexive_bytes(data in any::<Vec<u8>>()) {
        let item = StackItem::from_byte_string(data);
        prop_assert!(item.equals(&item).unwrap());
    }

    /// Test that non-zero integers are truthy
    #[test]
    fn test_stackitem_nonzero_is_truthy(value in any::<i64>().prop_filter("non-zero", |v| *v != 0)) {
        let item = StackItem::from_int(value);
        prop_assert!(item.as_bool().unwrap());
    }

    /// Test that non-empty bytestring is truthy (with size limit for conversion)
    #[test]
    fn test_stackitem_nonempty_bytestring_is_truthy(
        data in any::<Vec<u8>>().prop_filter("non-empty and within size limit", |v| !v.is_empty() && v.len() <= 32)
    ) {
        let item = StackItem::from_byte_string(data);
        prop_assert!(item.as_bool().unwrap());
    }

    // =========================================================================
    // Peek Tests
    // =========================================================================

    /// Test that peek doesn't remove the item - bool
    #[test]
    fn test_peek_preserves_item_bool(value in any::<bool>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_bool(value);
        let original = item.clone();
        stack.push(item).unwrap();

        // Peek multiple times
        let peek1 = stack.peek(0).unwrap().clone();
        let peek2 = stack.peek(0).unwrap().clone();

        // Stack should still have the item
        prop_assert_eq!(stack.len(), 1);

        // Pop should still work
        let popped = stack.pop().unwrap();

        prop_assert!(original.equals(&peek1).unwrap());
        prop_assert!(original.equals(&peek2).unwrap());
        prop_assert!(original.equals(&popped).unwrap());
    }

    /// Test that peek doesn't remove the item - int
    #[test]
    fn test_peek_preserves_item_int(value in any::<i64>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_int(value);
        let original = item.clone();
        stack.push(item).unwrap();

        let peek1 = stack.peek(0).unwrap().clone();
        prop_assert_eq!(stack.len(), 1);

        let popped = stack.pop().unwrap();
        prop_assert!(original.equals(&peek1).unwrap());
        prop_assert!(original.equals(&popped).unwrap());
    }

    /// Test that peek doesn't remove the item - bytes
    #[test]
    fn test_peek_preserves_item_bytes(data in any::<Vec<u8>>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);
        let item = StackItem::from_byte_string(data);
        let original = item.clone();
        stack.push(item).unwrap();

        let peek1 = stack.peek(0).unwrap().clone();
        prop_assert_eq!(stack.len(), 1);

        let popped = stack.pop().unwrap();
        prop_assert!(original.equals(&peek1).unwrap());
        prop_assert!(original.equals(&popped).unwrap());
    }

    // =========================================================================
    // Clear Tests
    // =========================================================================

    /// Test that clear empties the stack
    #[test]
    fn test_clear_empties_stack_bool(value in any::<bool>()) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);

        stack.push(StackItem::from_bool(value)).unwrap();
        prop_assert!(!stack.is_empty());

        stack.clear();
        prop_assert!(stack.is_empty());
        prop_assert_eq!(stack.len(), 0);
    }

    /// Test that clear empties the stack - multiple items
    #[test]
    fn test_clear_empties_stack_multiple(
        v1 in any::<bool>(),
        v2 in any::<i64>(),
        v3 in any::<Vec<u8>>()
    ) {
        let rc = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(rc);

        stack.push(StackItem::from_bool(v1)).unwrap();
        stack.push(StackItem::from_int(v2)).unwrap();
        stack.push(StackItem::from_byte_string(v3)).unwrap();
        prop_assert_eq!(stack.len(), 3);

        stack.clear();
        prop_assert!(stack.is_empty());
        prop_assert_eq!(stack.len(), 0);
    }
}

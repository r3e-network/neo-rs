//! Property-based tests for neo-primitives
//!
//! These tests use proptest to generate random inputs and verify properties
//! like serialization roundtrips, hash consistency, etc.

use neo_primitives::{UInt160, UInt256};
use proptest::prelude::*;

proptest! {
    // =========================================================================
    // UInt160 Serialization Roundtrip Tests
    // =========================================================================

    /// Test that UInt160 roundtrips correctly through bytes
    #[test]
    fn test_uint160_bytes_roundtrip(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        let result = uint.to_array();
        prop_assert_eq!(bytes, result);
    }

    /// Test that UInt160 roundtrips correctly through hex string
    #[test]
    fn test_uint160_hex_roundtrip(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        let hex = uint.to_hex_string();
        let parsed = UInt160::parse(&hex).unwrap();
        prop_assert_eq!(uint, parsed);
    }

    /// Test that UInt160 serialization is deterministic
    #[test]
    fn test_uint160_serde_deterministic(bytes in any::<[u8; 20]>()) {
        let uint1 = UInt160::from_bytes(&bytes).unwrap();
        let uint2 = UInt160::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint1, uint2);
        prop_assert_eq!(uint1.to_hex_string(), uint2.to_hex_string());
    }

    // =========================================================================
    // UInt256 Serialization Roundtrip Tests
    // =========================================================================

    /// Test that UInt256 roundtrips correctly through bytes
    #[test]
    fn test_uint256_bytes_roundtrip(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        let result = uint.to_array();
        prop_assert_eq!(bytes, result);
    }

    /// Test that UInt256 roundtrips correctly through hex string
    #[test]
    fn test_uint256_hex_roundtrip(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        let hex = uint.to_hex_string();
        let parsed = UInt256::parse(&hex).unwrap();
        prop_assert_eq!(uint, parsed);
    }

    /// Test that UInt256 serialization is deterministic
    #[test]
    fn test_uint256_serde_deterministic(bytes in any::<[u8; 32]>()) {
        let uint1 = UInt256::from_bytes(&bytes).unwrap();
        let uint2 = UInt256::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint1, uint2);
        prop_assert_eq!(uint1.to_hex_string(), uint2.to_hex_string());
    }

    // =========================================================================
    // Hash Consistency Tests
    // =========================================================================

    /// Test that UInt160 hash code is consistent
    #[test]
    fn test_uint160_hash_consistency(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        let hash1 = uint.get_hash_code();
        let hash2 = uint.get_hash_code();
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that UInt256 hash code is consistent
    #[test]
    fn test_uint256_hash_consistency(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        let hash1 = uint.get_hash_code();
        let hash2 = uint.get_hash_code();
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that equal values have equal hash codes
    #[test]
    fn test_uint160_equal_values_equal_hash(bytes in any::<[u8; 20]>()) {
        let uint1 = UInt160::from_bytes(&bytes).unwrap();
        let uint2 = UInt160::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint1.get_hash_code(), uint2.get_hash_code());
    }

    /// Test that equal values have equal hash codes
    #[test]
    fn test_uint256_equal_values_equal_hash(bytes in any::<[u8; 32]>()) {
        let uint1 = UInt256::from_bytes(&bytes).unwrap();
        let uint2 = UInt256::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint1.get_hash_code(), uint2.get_hash_code());
    }

    // =========================================================================
    // Ordering Property Tests
    // =========================================================================

    /// Test transitivity of UInt160 ordering
    #[test]
    fn test_uint160_ordering_transitive(
        a in any::<[u8; 20]>(),
        b in any::<[u8; 20]>(),
        c in any::<[u8; 20]>()
    ) {
        let uint_a = UInt160::from_bytes(&a).unwrap();
        let uint_b = UInt160::from_bytes(&b).unwrap();
        let uint_c = UInt160::from_bytes(&c).unwrap();

        if uint_a < uint_b && uint_b < uint_c {
            prop_assert!(uint_a < uint_c);
        }
        if uint_a > uint_b && uint_b > uint_c {
            prop_assert!(uint_a > uint_c);
        }
    }

    /// Test transitivity of UInt256 ordering
    #[test]
    fn test_uint256_ordering_transitive(
        a in any::<[u8; 32]>(),
        b in any::<[u8; 32]>(),
        c in any::<[u8; 32]>()
    ) {
        let uint_a = UInt256::from_bytes(&a).unwrap();
        let uint_b = UInt256::from_bytes(&b).unwrap();
        let uint_c = UInt256::from_bytes(&c).unwrap();

        if uint_a < uint_b && uint_b < uint_c {
            prop_assert!(uint_a < uint_c);
        }
        if uint_a > uint_b && uint_b > uint_c {
            prop_assert!(uint_a > uint_c);
        }
    }

    /// Test that ordering is antisymmetric
    #[test]
    fn test_uint160_ordering_antisymmetric(a in any::<[u8; 20]>(), b in any::<[u8; 20]>()) {
        let uint_a = UInt160::from_bytes(&a).unwrap();
        let uint_b = UInt160::from_bytes(&b).unwrap();

        if uint_a < uint_b {
            prop_assert!(uint_b >= uint_a);
        }
    }

    /// Test that ordering is antisymmetric
    #[test]
    fn test_uint256_ordering_antisymmetric(a in any::<[u8; 32]>(), b in any::<[u8; 32]>()) {
        let uint_a = UInt256::from_bytes(&a).unwrap();
        let uint_b = UInt256::from_bytes(&b).unwrap();

        if uint_a < uint_b {
            prop_assert!(uint_b >= uint_a);
        }
    }

    // =========================================================================
    // Zero Detection Tests
    // =========================================================================

    /// Test that is_zero correctly identifies zero values
    #[test]
    fn test_uint160_is_zero(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        let is_zero = bytes.iter().all(|&b| b == 0);
        prop_assert_eq!(uint.is_zero(), is_zero);
    }

    /// Test that is_zero correctly identifies zero values
    #[test]
    fn test_uint256_is_zero(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        let is_zero = bytes.iter().all(|&b| b == 0);
        prop_assert_eq!(uint.is_zero(), is_zero);
    }

    // =========================================================================
    // Equality Property Tests
    // =========================================================================

    /// Test that equality is reflexive
    #[test]
    fn test_uint160_equality_reflexive(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        prop_assert!(uint.equals(Some(&uint)));
    }

    /// Test that equality is reflexive
    #[test]
    fn test_uint256_equality_reflexive(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        prop_assert!(uint.equals(Some(&uint)));
    }

    /// Test that equality is symmetric
    #[test]
    fn test_uint160_equality_symmetric(a in any::<[u8; 20]>(), b in any::<[u8; 20]>()) {
        let uint_a = UInt160::from_bytes(&a).unwrap();
        let uint_b = UInt160::from_bytes(&b).unwrap();
        prop_assert_eq!(
            uint_a.equals(Some(&uint_b)),
            uint_b.equals(Some(&uint_a))
        );
    }

    /// Test that equality is symmetric
    #[test]
    fn test_uint256_equality_symmetric(a in any::<[u8; 32]>(), b in any::<[u8; 32]>()) {
        let uint_a = UInt256::from_bytes(&a).unwrap();
        let uint_b = UInt256::from_bytes(&b).unwrap();
        prop_assert_eq!(
            uint_a.equals(Some(&uint_b)),
            uint_b.equals(Some(&uint_a))
        );
    }

    /// Test that equals returns false for None
    #[test]
    fn test_uint160_equals_none(bytes in any::<[u8; 20]>()) {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        prop_assert!(!uint.equals(None));
    }

    /// Test that equals returns false for None
    #[test]
    fn test_uint256_equals_none(bytes in any::<[u8; 32]>()) {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        prop_assert!(!uint.equals(None));
    }
}

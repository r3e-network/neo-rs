//! Implementation of `UInt256`, a 256-bit unsigned integer.

use serde::{Deserialize, Serialize};

/// The length of `UInt256` values in bytes.
pub const UINT256_SIZE: usize = crate::constants::HASH_SIZE;

crate::uint_type! {
    /// Represents a 256-bit unsigned integer.
    #[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
    #[repr(C)]
    pub struct UInt256 {
        size = UINT256_SIZE;
        size_const = UINT256_SIZE;
        /// Zero value for `UInt256`.
        ZERO;
        as_ref = true;
        fields: [value1: u64, value2: u64, value3: u64, value4: u64];
    }
}

impl UInt256 {
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    /// Returns the Neo-compatible 32-bit hash code for this value.
    pub const fn hash_code(&self) -> i32 {
        let v1_hash = (self.value1 as i32) ^ ((self.value1 >> 32) as i32);
        let v2_hash = (self.value2 as i32) ^ ((self.value2 >> 32) as i32);
        let v3_hash = (self.value3 as i32) ^ ((self.value3 >> 32) as i32);
        let v4_hash = (self.value4 as i32) ^ ((self.value4 >> 32) as i32);
        v1_hash ^ v2_hash ^ v3_hash ^ v4_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_uint256_new() {
        let uint = UInt256::new();
        assert_eq!(uint.value1, 0);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);
    }

    #[test]
    fn test_uint256_from_bytes() {
        let mut data = [0u8; UINT256_SIZE];
        data[0] = 1;
        let uint = UInt256::from_bytes(&data).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        let result = UInt256::from_bytes(&[1u8; UINT256_SIZE - 1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_uint256_to_array() {
        let mut uint = UInt256::new();
        uint.value1 = 1;
        let array = uint.to_array();
        assert_eq!(array[0], 1);
        for &item in array.iter().take(UINT256_SIZE).skip(1) {
            assert_eq!(item, 0);
        }
    }

    #[test]
    fn test_uint256_parse() {
        let mut result = None;
        assert!(UInt256::try_parse(
            "0000000000000000000000000000000000000000000000000000000000000001",
            &mut result
        ));
        assert!(result.is_some());
        let uint = result.unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        let mut result = None;
        assert!(UInt256::try_parse(
            "0X0000000000000000000000000000000000000000000000000000000000000001",
            &mut result
        ));
        assert!(result.is_some());

        let result = UInt256::parse("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_uint256_to_hex_string() {
        let mut uint = UInt256::new();
        uint.value1 = 1;
        assert_eq!(
            uint.to_hex_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn test_uint256_ordering() {
        let mut uint1 = UInt256::new();
        uint1.value4 = 1;
        let mut uint2 = UInt256::new();
        uint2.value4 = 2;
        assert!(uint1 < uint2);
    }

    #[test]
    fn test_uint256_equals() {
        let mut uint1 = UInt256::new();
        uint1.value1 = 1;
        uint1.value2 = 2;
        uint1.value3 = 3;
        uint1.value4 = 4;

        let mut uint2 = UInt256::new();
        uint2.value1 = 1;
        uint2.value2 = 2;
        uint2.value3 = 3;
        uint2.value4 = 4;

        let mut uint3 = UInt256::new();
        uint3.value1 = 5;

        assert!(uint1.equals(Some(&uint2)));
        assert!(!uint1.equals(Some(&uint3)));
        assert!(!uint1.equals(None));
    }

    proptest! {
        #[test]
        fn test_roundtrip_from_bytes(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            prop_assert_eq!(bytes, uint.to_array());
        }

        #[test]
        fn test_parse_hex_string(hex in "[0-9a-fA-F]{64}") {
            let uint = UInt256::parse(&format!("0x{}", hex)).unwrap();
            let uint2 = UInt256::parse(&uint.to_hex_string()).unwrap();
            prop_assert_eq!(uint, uint2);
        }

        #[test]
        fn test_ordering_transitive(
            a in any::<[u8; UINT256_SIZE]>(),
            b in any::<[u8; UINT256_SIZE]>(),
            c in any::<[u8; UINT256_SIZE]>()
        ) {
            let a = UInt256::from_bytes(&a).unwrap();
            let b = UInt256::from_bytes(&b).unwrap();
            let c = UInt256::from_bytes(&c).unwrap();
            if a < b && b < c { prop_assert!(a < c); }
            if a > b && b > c { prop_assert!(a > c); }
        }

        #[test]
        fn test_is_zero_correct(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            prop_assert_eq!(uint.is_zero(), bytes.iter().all(|&b| b == 0));
        }

        #[test]
        fn test_as_ref_implementation(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            let ref_bytes: &[u8] = uint.as_ref();
            prop_assert_eq!(&bytes, ref_bytes);
        }

        #[test]
        fn test_get_hash_code_deterministic(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            prop_assert_eq!(uint.hash_code(), uint.hash_code());
        }

        #[test]
        fn test_equals_is_symmetric(
            a in any::<[u8; UINT256_SIZE]>(),
            b in any::<[u8; UINT256_SIZE]>()
        ) {
            let uint_a = UInt256::from_bytes(&a).unwrap();
            let uint_b = UInt256::from_bytes(&b).unwrap();
            prop_assert_eq!(uint_a.equals(Some(&uint_b)), uint_b.equals(Some(&uint_a)));
        }

        #[test]
        fn test_parse_with_0x_prefix(hex in "[0-9a-fA-F]{64}") {
            let with_prefix = UInt256::parse(&format!("0x{}", hex)).unwrap();
            let without_prefix = UInt256::parse(&hex).unwrap();
            prop_assert_eq!(with_prefix, without_prefix);
        }
    }
}

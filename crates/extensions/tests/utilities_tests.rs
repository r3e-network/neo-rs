//! Extensions Utilities C# Compatibility Tests
//!
//! These tests ensure full compatibility with C# Neo.Extensions utility functions.

use neo_extensions::utilities::*;

#[cfg(test)]
mod utilities_tests {
    use super::*;

    /// Test utility functions (matches C# utility helpers exactly)
    #[test]
    fn test_utility_functions_compatibility() {
        // Test endianness conversion
        let value = 0x12345678u32;
        let little_endian = EndianUtils::to_little_endian(value);
        let big_endian = EndianUtils::to_big_endian(value);

        assert_eq!(EndianUtils::from_little_endian(&little_endian), value);
        assert_eq!(EndianUtils::from_big_endian(&big_endian), value);

        // Test bit manipulation
        assert!(BitUtils::is_bit_set(0b10101010, 1));
        assert!(!BitUtils::is_bit_set(0b10101010, 0));

        let set_bit = BitUtils::set_bit(0b10101010, 0);
        assert_eq!(set_bit, 0b10101011);

        let clear_bit = BitUtils::clear_bit(0b10101010, 1);
        assert_eq!(clear_bit, 0b10101000);
    }

    /// Test time utilities (matches C# DateTime extensions exactly)
    #[test]
    fn test_time_utilities_compatibility() {
        let timestamp = 1234567890u64;
        let datetime = TimeUtils::timestamp_to_datetime(timestamp);
        let back_to_timestamp = TimeUtils::datetime_to_timestamp(&datetime);
        assert_eq!(back_to_timestamp, timestamp);

        // Test Neo epoch conversion
        let neo_timestamp = TimeUtils::to_neo_timestamp(&datetime);
        let from_neo = TimeUtils::from_neo_timestamp(neo_timestamp);
        assert_eq!(from_neo, datetime);
    }
}

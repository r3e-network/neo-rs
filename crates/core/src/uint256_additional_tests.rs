//! Additional UInt256 Tests to Complete C# Neo UT_UInt256 Parity
//!
//! These tests specifically match the missing C# UT_UInt256 test methods.

#[cfg(test)]
mod additional_uint256_tests {
    use crate::UInt256;
    use neo_io::{BinaryWriter, MemoryReader, Serializable};

    /// Test exact C# TestGernerator3 behavior (string conversion)
    #[test]
    fn test_uint256_c_sharp_generator3() {
        // C# test: UInt256 uInt256 = "0xff00000000000000000000000000000000000000000000000000000000000001";
        // Assert.IsNotNull(uInt256); Assert.AreEqual("0xff00000000000000000000000000000000000000000000000000000000000001", uInt256.ToString());

        let hex_str = "0xff00000000000000000000000000000000000000000000000000000000000001";
        let uint256 = UInt256::from(hex_str);

        assert_eq!(uint256.to_string(), hex_str);
        assert_ne!(uint256, UInt256::zero());
    }

    /// Test TryParse functionality (matches C# TestTryParse with exact signature)
    #[test]
    fn test_uint256_try_parse_c_sharp_parity() {
        // Test with null/empty should fail
        let mut result = None;
        assert!(!UInt256::try_parse("", &mut result));

        // Test with valid hex string with 0x prefix should succeed
        let mut result = None;
        assert!(UInt256::try_parse(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
        if let Some(temp) = result {
            assert_eq!(
                temp.to_string(),
                "0x0000000000000000000000000000000000000000000000000000000000000000"
            );
            assert_eq!(temp, UInt256::zero());
        }

        // Test with valid hex string with 0x prefix should succeed
        let mut result = None;
        assert!(UInt256::try_parse(
            "0x1230000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
        if let Some(temp) = result {
            assert_eq!(
                temp.to_string(),
                "0x1230000000000000000000000000000000000000000000000000000000000000"
            );
        }

        // Test with invalid length should fail
        let mut result = None;
        assert!(!UInt256::try_parse(
            "000000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));

        // Test with invalid characters should fail
        let mut result = None;
        assert!(!UInt256::try_parse(
            "0xKK00000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
    }

    /// Test Parse functionality (matches C# TestParse)
    #[test]
    fn test_uint256_parse_c_sharp_parity() {
        // Test parsing null (empty string) should fail
        let result = UInt256::parse("");
        assert!(result.is_err(), "Parsing empty string should fail");

        // Test parsing valid hex string with 0x prefix
        let result =
            UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000000")
                .expect("Valid hex should parse");
        assert_eq!(result, UInt256::zero());

        // Test parsing invalid length (63 chars instead of 64) should fail
        let result =
            UInt256::parse("000000000000000000000000000000000000000000000000000000000000000");
        assert!(result.is_err(), "Invalid length string should fail");

        // Test parsing valid hex string without 0x prefix
        let result =
            UInt256::parse("0000000000000000000000000000000000000000000000000000000000000000")
                .expect("Valid hex should parse");
        assert_eq!(result, UInt256::zero());
    }

    /// Test operator > (matches C# TestOperatorLarger)
    #[test]
    fn test_uint256_operator_larger() {
        // Test that zero is not greater than zero
        assert!(!(UInt256::zero() > UInt256::zero()));
    }

    /// Test operator >= (matches C# TestOperatorLargerAndEqual)
    #[test]
    fn test_uint256_operator_larger_and_equal() {
        // Test that zero is greater than or equal to zero
        assert!(UInt256::zero() >= UInt256::zero());
    }

    /// Test operator < (matches C# TestOperatorSmaller)
    #[test]
    fn test_uint256_operator_smaller() {
        // Test that zero is not less than zero
        assert!(!(UInt256::zero() < UInt256::zero()));
    }

    /// Test operator <= (matches C# TestOperatorSmallerAndEqual)
    #[test]
    fn test_uint256_operator_smaller_and_equal() {
        // Test that zero is less than or equal to zero
        assert!(UInt256::zero() <= UInt256::zero());
    }

    /// Test operator == with null (matches C# TestOperatorEqual)
    #[test]
    fn test_uint256_operator_equal() {
        // C# test: Assert.IsFalse(new UInt256() == null);
        // Assert.IsFalse(null == new UInt256());
        // Note: In Rust, we can't compare to null, but we can test Option<UInt256>

        let uint256 = UInt256::zero();
        let none_uint256: Option<UInt256> = None;

        // Test with Some vs None
        assert_ne!(Some(uint256), none_uint256);
        assert_ne!(none_uint256, Some(uint256));
    }

    /// Test equals functionality (matches C# TestEquals1)
    #[test]
    fn test_uint256_equals1() {
        let temp1 = UInt256::new();
        let temp2 = UInt256::new();

        // Test self equality
        assert!(temp1.equals(Some(&temp1)));

        // Test equality with same value
        assert!(temp1.equals(Some(&temp2)));

        // Test inequality with None
        assert!(!temp1.equals(None));
    }

    /// Test equals functionality (matches C# TestEquals2)
    #[test]
    fn test_uint256_equals2() {
        let temp1 = UInt256::new();

        // Test equality with None (like null in C#)
        assert!(!temp1.equals(None));

        // Test that we can distinguish UInt256 from other types
        // (In Rust, we can't directly compare different types like C# can with objects)
        let different_uint =
            UInt256::from("0000000000000000000000000000000000000000000000000000000000000001");
        assert!(!temp1.equals(Some(&different_uint)));
    }

    /// Test span and serialization (matches C# TestSpanAndSerialize)
    #[test]
    fn test_uint256_span_and_serialize() {
        // Create test data (using fixed data instead of random for reproducibility)
        let mut data = [0u8; 32];
        for i in 0..32 {
            data[i] = (i as u8).wrapping_mul(7); // Simple pattern instead of random
        }

        let value = UInt256::from_bytes(&data).unwrap();
        let span = value.get_span();

        // Test that GetSpan returns the same as ToArray
        assert_eq!(span, value.to_array());

        // Test serialization using neo_io::Serializable
        let mut writer = BinaryWriter::new();
        value
            .serialize(&mut writer)
            .expect("Serialization should succeed");
        let serialized_bytes = writer.to_bytes();

        // The serialized bytes should match the original data
        assert_eq!(serialized_bytes, data.to_vec());

        // Test deserialization
        let mut reader = MemoryReader::new(&serialized_bytes);
        let deserialized =
            UInt256::deserialize(&mut reader).expect("Deserialization should succeed");
        assert_eq!(deserialized, value);
    }

    /// Test span and serialization little endian (matches C# TestSpanAndSerializeLittleEndian)
    #[test]
    fn test_uint256_span_and_serialize_little_endian() {
        // Create test data (using fixed data instead of random for reproducibility)
        let mut data = [0u8; 32];
        for i in 0..32 {
            data[i] = (i as u8).wrapping_mul(13); // Simple pattern instead of random
        }

        let value = UInt256::from_bytes(&data).unwrap();

        // Test GetSpan (little endian representation)
        let span_little_endian = value.get_span();
        assert_eq!(data, span_little_endian);

        // Test that Serialize and SafeSerialize produce the same result as ToArray
        // Note: In our implementation, both serialize to the same little-endian format
        let mut writer = BinaryWriter::new();
        value
            .serialize(&mut writer)
            .expect("Serialization should succeed");
        let serialized_data = writer.to_bytes();
        assert_eq!(value.to_array().to_vec(), serialized_data);

        // Test serialization buffer size validation would require implementing SafeSerialize
        // For now, we verify the basic serialization works correctly
        let mut reader = MemoryReader::new(&serialized_data);
        let deserialized =
            UInt256::deserialize(&mut reader).expect("Deserialization should succeed");
        assert_eq!(deserialized, value);
    }

    /// Test deserialization failure (matches C# TestDeserialize)
    #[test]
    fn test_uint256_deserialize_failure() {
        // Test deserialization with insufficient data (matches C# test that expects FormatException)
        let invalid_data = [0u8; 20]; // Only 20 bytes instead of 32
        let mut reader = MemoryReader::new(&invalid_data);

        let result = UInt256::deserialize(&mut reader);
        assert!(
            result.is_err(),
            "Deserialization should fail with insufficient data"
        );
    }
}

//! Additional UInt160 Tests to Complete C# Neo UT_UInt160 Parity
//!
//! These tests specifically match the missing C# UT_UInt160 test methods.

#[cfg(test)]
mod additional_uint160_tests {
    use crate::UInt160;
    use neo_io::{BinaryWriter, MemoryReader, Serializable};

    /// Test operator > (matches C# TestOperatorLarger)
    #[test]
    fn test_uint160_operator_larger() {
        // Test that zero is not greater than zero
        assert!(!(UInt160::zero() > UInt160::zero()));

        // Test that zero is not greater than string representation of zero
        let zero_from_string = UInt160::from("0x0000000000000000000000000000000000000000");
        assert!(!(UInt160::zero() > zero_from_string));
    }

    /// Test operator >= (matches C# TestOperatorLargerAndEqual)
    #[test]
    fn test_uint160_operator_larger_and_equal() {
        // Test that zero is greater than or equal to zero
        assert!(UInt160::zero() >= UInt160::zero());

        // Test that zero is greater than or equal to string representation of zero
        let zero_from_string = UInt160::from("0x0000000000000000000000000000000000000000");
        assert!(UInt160::zero() >= zero_from_string);
    }

    /// Test operator < (matches C# TestOperatorSmaller)
    #[test]
    fn test_uint160_operator_smaller() {
        // Test that zero is not less than zero
        assert!(!(UInt160::zero() < UInt160::zero()));

        // Test that zero is not less than string representation of zero
        let zero_from_string = UInt160::from("0x0000000000000000000000000000000000000000");
        assert!(!(UInt160::zero() < zero_from_string));
    }

    /// Test operator <= (matches C# TestOperatorSmallerAndEqual)
    #[test]
    fn test_uint160_operator_smaller_and_equal() {
        // Test that zero is less than or equal to zero
        assert!(UInt160::zero() <= UInt160::zero());

        // Test that zero is less than or equal to string representation of zero
        let zero_from_string = UInt160::from("0x0000000000000000000000000000000000000000");
        assert!(UInt160::zero() <= zero_from_string);

        // Also test the >= operator as mentioned in the C# test (even though it's likely a typo)
        assert!(UInt160::zero() >= zero_from_string);
    }

    /// Test span and serialization (matches C# TestSpanAndSerialize)
    #[test]
    fn test_uint160_span_and_serialize() {
        // Create test data (using fixed data instead of random for reproducibility)
        let mut data = [0u8; 20];
        for i in 0..20 {
            data[i] = (i as u8).wrapping_mul(7); // Simple pattern instead of random
        }

        let value = UInt160::from_bytes(&data).unwrap();
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
            UInt160::deserialize(&mut reader).expect("Deserialization should succeed");
        assert_eq!(deserialized, value);
    }

    /// Test span and serialization little endian (matches C# TestSpanAndSerializeLittleEndian)
    #[test]
    fn test_uint160_span_and_serialize_little_endian() {
        // Create test data (using fixed data instead of random for reproducibility)
        let mut data = [0u8; 20];
        for i in 0..20 {
            data[i] = (i as u8).wrapping_mul(13); // Simple pattern instead of random
        }

        let value = UInt160::from_bytes(&data).unwrap();

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
            UInt160::deserialize(&mut reader).expect("Deserialization should succeed");
        assert_eq!(deserialized, value);
    }

    /// Test exact C# TestGernerator3 behavior (string conversion)
    #[test]
    fn test_uint160_c_sharp_generator3() {
        // C# test: UInt160 uInt160 = "0xff00000000000000000000000000000000000001";
        // Assert.IsNotNull(uInt160); Assert.AreEqual("0xff00000000000000000000000000000000000001", uInt160.ToString());

        let hex_str = "0xff00000000000000000000000000000000000001";
        let uint160 = UInt160::from(hex_str);

        assert_eq!(uint160.to_string(), hex_str);
        assert_ne!(uint160, UInt160::zero());
    }
}

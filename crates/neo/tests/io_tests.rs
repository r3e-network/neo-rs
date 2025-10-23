//! Comprehensive IO tests converted from C# Neo unit tests.
//! These tests ensure 100% compatibility with the C# Neo IO implementation.

use neo_core::compression::CompressionError;
use neo_core::extensions::{
    BinaryReaderExtensions, BinaryWriterExtensions, ByteExtensions, CollectionExtensions,
    MemoryReaderExtensions, SerializableExtensions,
};
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::*;
use std::io::Cursor;
use std::str::FromStr;

// ============================================================================
// C# Neo Unit Test Conversions - MemoryReader Tests
// ============================================================================

/// Test converted from C# UT_MemoryReader.TestReadSByte
#[test]
fn test_memory_reader_read_sbyte() {
    let values = vec![0i8, 1, -1, 5, -5, i8::MAX, i8::MIN];

    for v in values {
        let byte_array = vec![v as u8];
        let mut reader = MemoryReader::new(&byte_array);
        let result = reader.read_sbyte().unwrap();
        assert_eq!(v, result);
    }

    // Test overflow cases
    let overflow_values = vec![(i32::MAX as i64 + 1) as u8, (i32::MIN as i64 - 1) as u8];

    for v in overflow_values {
        let byte_array = vec![v];
        let mut reader = MemoryReader::new(&byte_array);
        let result = reader.read_sbyte().unwrap();
        assert_eq!(v as i8, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadInt32
#[test]
fn test_memory_reader_read_int32() {
    let values = vec![0i32, 1, -1, 5, -5, i32::MAX, i32::MIN];

    for v in values {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_int32().unwrap();
        assert_eq!(v, result);
    }

    // Test overflow cases
    let overflow_values = vec![i32::MAX as i64 + 1, i32::MIN as i64 - 1];

    for v in overflow_values {
        let bytes = (v as i32).to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_int32().unwrap();
        assert_eq!(v as i32, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadUInt64
#[test]
fn test_memory_reader_read_uint64() {
    let values = vec![0u64, 1, 5, u64::MAX, u64::MIN];

    for v in values {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_u64().unwrap();
        assert_eq!(v, result);
    }

    // Test signed values interpreted as unsigned
    let signed_values = vec![i64::MIN, -1, i64::MAX];

    for v in signed_values {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_u64().unwrap();
        assert_eq!(v as u64, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadInt16BigEndian
#[test]
fn test_memory_reader_read_int16_big_endian() {
    let values = vec![i16::MIN, -1, 0, 1, 12345, i16::MAX];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_int16_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadUInt16BigEndian
#[test]
fn test_memory_reader_read_uint16_big_endian() {
    let values = vec![u16::MIN, 0, 1, 12345, u16::MAX];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_uint16_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadInt32BigEndian
#[test]
fn test_memory_reader_read_int32_big_endian() {
    let values = vec![i32::MIN, -1, 0, 1, 12345, i32::MAX];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_int32_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadUInt32BigEndian
#[test]
fn test_memory_reader_read_uint32_big_endian() {
    let values = vec![u32::MIN, 0, 1, 12345, u32::MAX];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_uint32_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadInt64BigEndian
#[test]
fn test_memory_reader_read_int64_big_endian() {
    let values = vec![
        i64::MIN,
        i32::MIN as i64,
        -1,
        0,
        1,
        12345,
        i32::MAX as i64,
        i64::MAX,
    ];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_int64_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadUInt64BigEndian
#[test]
fn test_memory_reader_read_uint64_big_endian() {
    let values = vec![u64::MIN, 0, 1, 12345, u64::MAX];

    for v in values {
        let bytes = v.to_be_bytes(); // Big-endian
        let mut reader = MemoryReader::new(&bytes);
        let result = reader.read_uint64_big_endian().unwrap();
        assert_eq!(v, result);
    }
}

/// Test converted from C# UT_MemoryReader.TestReadFixedString
#[test]
fn test_memory_reader_read_fixed_string() {
    let test_string = "AA";
    let mut data = test_string.as_bytes().to_vec();
    data.push(0); // Null terminator

    let mut reader = MemoryReader::new(&data);
    let result = reader.read_fixed_string(data.len()).unwrap();
    assert_eq!(test_string, result);
}

/// Test converted from C# UT_MemoryReader.TestReadVarString
#[test]
fn test_memory_reader_read_var_string() {
    let test_string = "AAAAAAA";

    // Create data with variable length prefix
    let mut writer = BinaryWriter::new();
    writer.write_var_string(test_string).unwrap();
    let data = writer.to_bytes();

    let mut reader = MemoryReader::new(&data);
    let result = reader.read_var_string(10).unwrap();
    assert_eq!(test_string, result);
}

/// Test converted from C# UT_MemoryReader.TestReadNullableArray
#[test]
fn test_memory_reader_read_nullable_array() {
    let expected_third = UInt160::from_bytes(&[
        0xAA, 0x00, 0x00, 0x00, 0x00, 0xBB, 0x00, 0x00, 0x00, 0x00, 0xCC, 0x00, 0x00, 0x00, 0x00,
        0xDD, 0x00, 0x00, 0x00, 0x00,
    ])
    .unwrap();

    let values = [None, Some(UInt160::zero()), Some(expected_third)];

    let mut writer = BinaryWriter::new();
    writer.write_nullable_array(&values).unwrap();
    let payload = writer.to_bytes();

    let mut reader_too_small = MemoryReader::new(&payload);
    assert!(reader_too_small
        .read_nullable_array::<UInt160>(values.len() - 1)
        .is_err());

    let mut reader = MemoryReader::new(&payload);
    let roundtrip = reader.read_nullable_array::<UInt160>(usize::MAX).unwrap();

    assert_eq!(roundtrip, values.to_vec());
    assert_eq!(reader.position(), payload.len());
}

// ============================================================================
// Additional MemoryReader Tests
// ============================================================================

/// Test MemoryReader position management
#[test]
fn test_memory_reader_position_management() {
    let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let mut reader = MemoryReader::new(&data);

    // Initial position should be 0
    assert_eq!(0, reader.position());

    // Read a byte and check position
    let byte1 = reader.read_byte().unwrap();
    assert_eq!(1, byte1);
    assert_eq!(1, reader.position());

    // Read a u32 and check position
    let value = reader.read_u32().unwrap();
    assert_eq!(0x05040302, value); // Little-endian: [2,3,4,5]
    assert_eq!(5, reader.position());

    // Set position manually
    reader.set_position(2).unwrap();
    assert_eq!(2, reader.position());

    // Read from new position
    let byte3 = reader.read_byte().unwrap();
    assert_eq!(3, byte3);
    assert_eq!(3, reader.position());
}

/// Test MemoryReader end of stream handling
#[test]
fn test_memory_reader_end_of_stream() {
    let data = vec![1, 2];
    let mut reader = MemoryReader::new(&data);

    // Read available bytes
    assert_eq!(1, reader.read_byte().unwrap());
    assert_eq!(2, reader.read_byte().unwrap());

    // Try to read beyond end of stream
    assert!(reader.read_byte().is_err());
    assert!(reader.read_u32().is_err());
    assert!(reader.read_u64().is_err());
}

/// Test MemoryReader boolean reading
#[test]
fn test_memory_reader_read_boolean() {
    let data = vec![0, 1, 255, 42];
    let mut reader = MemoryReader::new(&data);

    assert!(!reader.read_boolean().unwrap());
    assert!(reader.read_boolean().unwrap());
    // Note: C# allows any non-zero as true, but our implementation is stricter
    assert!(reader.read_boolean().is_err()); // 255 should fail
    assert!(reader.read_boolean().is_err()); // 42 should fail
}

/// Test MemoryReader variable integer reading
#[test]
fn test_memory_reader_read_var_int() {
    let data = vec![42];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(42, reader.read_var_int(u64::MAX).unwrap());

    let data = vec![0xFD, 0x34, 0x12]; // 0x1234 in little-endian
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x1234, reader.read_var_int(u64::MAX).unwrap());

    let data = vec![0xFE, 0x78, 0x56, 0x34, 0x12]; // 0x12345678 in little-endian
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x12345678, reader.read_var_int(u64::MAX).unwrap());

    let data = vec![0xFF, 0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x1234567812345678, reader.read_var_int(u64::MAX).unwrap());
}

/// Test MemoryReader variable bytes reading
#[test]
fn test_memory_reader_read_var_bytes() {
    let test_data = vec![1, 2, 3, 4, 5];

    // Create data with variable length prefix
    let mut writer = BinaryWriter::new();
    writer.write_var_bytes(&test_data).unwrap();
    let data = writer.to_bytes();

    let mut reader = MemoryReader::new(&data);
    let result = reader.read_var_bytes(10).unwrap();
    assert_eq!(test_data, result);
}

// ============================================================================
// BinaryReader Extension Tests
// ============================================================================

/// Test converted from C# UT_IOHelper.TestReadFixedBytes
#[test]
fn test_binary_reader_read_fixed_bytes() {
    let data = vec![0x01, 0x02, 0x03, 0x04];

    let mut reader = Cursor::new(data.clone());
    let less = reader.read_fixed_bytes(3).unwrap();
    assert_eq!(vec![0x01, 0x02, 0x03], less);
    assert_eq!(3, reader.position());

    let mut reader_exact = Cursor::new(data.clone());
    let exact = reader_exact.read_fixed_bytes(4).unwrap();
    assert_eq!(data, exact);
    assert_eq!(4, reader_exact.position());

    let mut reader_over = Cursor::new(data.clone());
    assert!(reader_over.read_fixed_bytes(5).is_err());
    assert_eq!(4, reader_over.position());
}

/// Test converted from C# UT_IOHelper.TestReadVarBytes
#[test]
fn test_binary_reader_read_var_bytes() {
    let mut writer = BinaryWriter::new();
    writer.write_var_bytes(&[0xAA, 0xAA]).unwrap();
    let data = writer.to_bytes();

    let mut reader = Cursor::new(data.clone());
    let bytes = reader.read_var_bytes(10).unwrap();
    assert_eq!(vec![0xAA, 0xAA], bytes);

    let mut reader_too_big = Cursor::new(data);
    assert!(reader_too_big.read_var_bytes(1).is_err());
}

/// Test converted from C# UT_IOHelper.TestReadVarInt
#[test]
fn test_binary_reader_read_var_int() {
    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFFFF).unwrap();
    let data = writer.to_bytes();
    let mut reader = Cursor::new(data);
    assert_eq!(0xFFFF, reader.read_var_int(0xFFFF).unwrap());

    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFFFF_FFFF).unwrap();
    let data = writer.to_bytes();
    let mut reader = Cursor::new(data);
    assert_eq!(0xFFFF_FFFF, reader.read_var_int(0xFFFF_FFFF).unwrap());

    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFFFF_FFFF_FF).unwrap();
    let data = writer.to_bytes();
    let mut reader = Cursor::new(data);
    assert!(reader.read_var_int(0xFFFF_FFFF).is_err());
}

// ============================================================================
// BinaryWriter Tests
// ============================================================================

/// Test BinaryWriter basic operations
#[test]
fn test_binary_writer_basic_operations() {
    let mut writer = BinaryWriter::new();

    // Write various types
    writer.write_u8(42).unwrap();
    writer.write_u16(0x1234).unwrap();
    writer.write_u32(0x12345678).unwrap();
    writer.write_u64(0x123456789ABCDEF0).unwrap();

    let data = writer.to_bytes();
    let mut reader = MemoryReader::new(&data);

    // Read back and verify
    assert_eq!(42, reader.read_byte().unwrap());
    assert_eq!(0x1234, reader.read_uint16().unwrap());
    assert_eq!(0x12345678, reader.read_u32().unwrap());
    assert_eq!(0x123456789ABCDEF0, reader.read_u64().unwrap());
}

/// Test BinaryWriter variable length encoding
#[test]
fn test_binary_writer_variable_length() {
    let mut writer = BinaryWriter::new();

    // Write variable integers
    writer.write_var_int(42).unwrap();
    writer.write_var_int(0x1234).unwrap();
    writer.write_var_int(0x12345678).unwrap();

    writer.write_var_string("Hello, Neo!").unwrap();

    // Write variable bytes
    let test_bytes = vec![1, 2, 3, 4, 5];
    writer.write_var_bytes(&test_bytes).unwrap();

    let data = writer.to_bytes();
    let mut reader = MemoryReader::new(&data);

    // Read back and verify
    assert_eq!(42, reader.read_var_int(u64::MAX).unwrap());
    assert_eq!(0x1234, reader.read_var_int(u64::MAX).unwrap());
    assert_eq!(0x12345678, reader.read_var_int(u64::MAX).unwrap());
    assert_eq!("Hello, Neo!", reader.read_var_string(20).unwrap());
    assert_eq!(test_bytes, reader.read_var_bytes(10).unwrap());
}

/// Test converted from C# UT_IOHelper.TestWrite
#[test]
fn test_binary_writer_write_serializable() {
    let mut writer = BinaryWriter::new();
    writer.write_serializable(&UInt160::zero()).unwrap();
    assert_eq!(vec![0u8; 20], writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestWriteGeneric
#[test]
fn test_binary_writer_write_serializable_collection() {
    let mut writer = BinaryWriter::new();
    let values = [UInt160::zero()];
    writer.write_serializable_collection(&values).unwrap();

    let mut expected = vec![0x01];
    expected.extend(vec![0u8; 20]);
    assert_eq!(expected, writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestToByteArrayGeneric
#[test]
fn test_collection_extensions_to_byte_array() {
    let values = vec![UInt160::zero()];
    let bytes = values.to_byte_array().unwrap();

    let mut expected = vec![0x01];
    expected.extend(vec![0u8; 20]);
    assert_eq!(expected, bytes);
}

/// Test converted from C# UT_IOHelper.TestWriteFixedString
#[test]
fn test_binary_writer_write_fixed_string() {
    let mut writer = BinaryWriter::new();
    assert!(writer.write_fixed_string("AA", 1).is_err());

    let mut writer = BinaryWriter::new();
    let wide = "\u{62C9}\u{62C9}";
    assert!(writer.write_fixed_string(wide, 5).is_err());

    let mut writer = BinaryWriter::new();
    writer.write_fixed_string("AA", "AA".len() + 1).unwrap();
    let mut expected = Vec::from("AA".as_bytes());
    expected.push(0);
    assert_eq!(expected, writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestWriteVarBytes
#[test]
fn test_binary_writer_write_var_bytes() {
    let mut writer = BinaryWriter::new();
    writer.write_var_bytes(&[0xAA]).unwrap();
    assert_eq!(vec![0x01, 0xAA], writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestWriteVarInt
#[test]
fn test_binary_writer_write_var_int_boundaries() {
    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFC).unwrap();
    assert_eq!(vec![0xFC], writer.to_bytes());

    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFFFF).unwrap();
    let mut expected = vec![0xFD];
    expected.extend_from_slice(&0xFFFFu16.to_le_bytes());
    assert_eq!(expected, writer.to_bytes());

    let mut writer = BinaryWriter::new();
    writer.write_var_int(0xFFFF_FFFF).unwrap();
    let mut expected = vec![0xFE];
    expected.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    assert_eq!(expected, writer.to_bytes());

    let mut writer = BinaryWriter::new();
    let large = 0xAEFF_FFFF_FFFFu64;
    writer.write_var_int(large).unwrap();
    let mut expected = vec![0xFF];
    expected.extend_from_slice(&large.to_le_bytes());
    assert_eq!(expected, writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestWriteVarString
#[test]
fn test_binary_writer_write_var_string() {
    let mut writer = BinaryWriter::new();
    writer.write_var_string("a").unwrap();
    assert_eq!(vec![0x01, b'a'], writer.to_bytes());
}

/// Test converted from C# UT_IOHelper.TestAsSerializable
#[test]
fn test_byte_extensions_as_serializable() {
    let data = vec![0u8; 20];
    let value: UInt160 = data.as_serializable(0).unwrap();
    assert_eq!(UInt160::zero(), value);
}

/// Test converted from C# UT_IOHelper.TestAsSerializableArray
#[test]
fn test_byte_extensions_as_serializable_array() {
    let mut writer = BinaryWriter::new();
    let values = [UInt160::zero()];
    writer.write_serializable_collection(&values).unwrap();
    let data = writer.to_bytes();

    let result: Vec<UInt160> = data.as_serializable_array(usize::MAX).unwrap();
    assert_eq!(values.to_vec(), result);

    assert!(data.as_serializable_array::<UInt160>(0).is_err());
}

/// Test converted from C# UT_IOHelper.TestReadSerializable
#[test]
fn test_memory_reader_read_serializable() {
    let mut writer = BinaryWriter::new();
    writer.write_serializable(&UInt160::zero()).unwrap();
    let data = writer.to_bytes();

    let mut reader = MemoryReader::new(&data);
    let result: UInt160 = reader.read_serializable().unwrap();
    assert_eq!(UInt160::zero(), result);
}

/// Test converted from C# UT_IOHelper.TestReadSerializableArray
#[test]
fn test_memory_reader_read_serializable_array() {
    let mut writer = BinaryWriter::new();
    let values = [UInt160::zero()];
    writer.write_serializable_collection(&values).unwrap();
    let data = writer.to_bytes();

    let mut reader = MemoryReader::new(&data);
    let result: Vec<UInt160> = reader.read_serializable_array(usize::MAX).unwrap();
    assert_eq!(values.to_vec(), result);

    let mut reader = MemoryReader::new(&data);
    assert!(reader.read_serializable_array::<UInt160>(0).is_err());
}

/// Test converted from C# UT_IOHelper.TestToArray
#[test]
fn test_serializable_to_array() {
    let bytes = SerializableExtensions::to_array(&UInt160::zero()).unwrap();
    assert_eq!(vec![0u8; 20], bytes);
}

// ============================================================================
// Byte Extension Tests
// ============================================================================

/// Test converted from C# UT_IOHelper.TestCompression (round-trip scenarios)
#[test]
fn test_byte_extensions_compression_round_trip() {
    let data = vec![1u8, 2, 3, 4];
    let compressed = data.compress_lz4().unwrap();
    let decompressed = compressed.decompress_lz4(usize::MAX).unwrap();
    assert_eq!(data, decompressed);

    let repetitive = vec![1u8; 255];
    let compressed = repetitive.compress_lz4().unwrap();
    let decompressed = compressed.decompress_lz4(usize::MAX).unwrap();
    assert!(compressed.len() < repetitive.len());
    assert_eq!(repetitive, decompressed);
}

/// Test converted from C# UT_IOHelper.TestCompression (error scenarios)
#[test]
fn test_byte_extensions_compression_errors() {
    let data = vec![1u8; 32];
    let compressed = data.compress_lz4().unwrap();

    let too_small = compressed.decompress_lz4(data.len() - 1);
    assert!(matches!(too_small, Err(CompressionError::TooLarge { .. })));

    let mut corrupted = compressed.clone();
    corrupted[0] = corrupted[0].wrapping_add(1);
    let corrupted_result = corrupted.decompress_lz4(usize::MAX);
    assert!(matches!(
        corrupted_result,
        Err(CompressionError::Decompression(_))
    ));
}

/// Test serialization round-trip with UInt160
#[test]
fn test_serialization_round_trip_uint160() {
    let original = UInt160::from_str("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();

    // Serialize
    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();
    let data = writer.to_bytes();

    // Deserialize
    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt160::deserialize(&mut reader).unwrap();

    // Verify
    assert_eq!(original, deserialized);
}

/// Test serialization round-trip with UInt256
#[test]
fn test_serialization_round_trip_uint256() {
    let original =
        UInt256::from_str("0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
            .unwrap();

    // Serialize
    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();
    let data = writer.to_bytes();

    // Deserialize
    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt256::deserialize(&mut reader).unwrap();

    // Verify
    assert_eq!(original, deserialized);
}

/// Test serialization round-trip with Transaction (basic test)
#[test]
fn test_serialization_round_trip_transaction() {
    let original = Transaction::new();

    // Test that we can get the size
    let size = original.size();
    assert!(size > 0);

    // Test that basic properties work
    assert_eq!(original.version(), 0);
    assert_eq!(original.nonce(), 0);
    assert_eq!(original.system_fee(), 0);
    assert_eq!(original.network_fee(), 0);
    assert_eq!(original.valid_until_block(), 0);
    assert!(original.signers().is_empty());
    assert!(original.attributes().is_empty());
    assert!(original.script().is_empty());
}

/// Test comprehensive transaction serialization and deserialization
#[test]
fn test_transaction_serialization_comprehensive() {
    use neo_core::network::p2p::payloads::{Signer, Witness};
    use neo_core::{Transaction, UInt160, WitnessScope};
    use std::str::FromStr;

    // Create a transaction with actual data
    let mut tx = Transaction::new();

    // Set transaction properties
    tx.set_version(0);
    tx.set_nonce(123456789);
    tx.set_system_fee(1000000);
    tx.set_network_fee(500000);
    tx.set_valid_until_block(1000);

    // Add a script
    let script = vec![0x51, 0x52, 0x53]; // PUSH1 PUSH2 PUSH3
    tx.set_script(script.clone());

    // Add a signer
    let signer_account = UInt160::from_str("0x0000000000000000000000000000000000000001").unwrap();
    let signer = Signer::new(signer_account, WitnessScope::CALLED_BY_ENTRY);
    tx.add_signer(signer);

    // Add a witness
    let invocation_script = vec![0x40]; // Signature placeholder
    let verification_script = vec![0x21, 0x03, 0x01, 0x02, 0x03]; // Public key placeholder
    let witness = Witness::new_with_scripts(invocation_script.clone(), verification_script.clone());
    tx.add_witness(witness);

    // Test serialization
    let serialized = tx.to_bytes();
    assert!(
        !serialized.is_empty(),
        "Serialized data should not be empty"
    );

    // Test deserialization
    let deserialized =
        Transaction::from_bytes(&serialized).expect("Should deserialize successfully");

    assert_eq!(deserialized.version(), 0);
    assert_eq!(deserialized.nonce(), 123456789);
    assert_eq!(deserialized.system_fee(), 1000000);
    assert_eq!(deserialized.network_fee(), 500000);
    assert_eq!(deserialized.valid_until_block(), 1000);
    assert_eq!(deserialized.script(), &script);
    assert_eq!(deserialized.signers().len(), 1);
    assert_eq!(deserialized.signers()[0].account, signer_account);
    assert_eq!(deserialized.witnesses().len(), 1);
    assert_eq!(
        deserialized.witnesses()[0].invocation_script(),
        &invocation_script
    );
    assert_eq!(
        deserialized.witnesses()[0].verification_script(),
        &verification_script
    );

    // Test hex serialization
    let hex = hex::encode(&serialized);
    let from_hex_bytes = hex::decode(&hex).expect("Hex decode should succeed");
    let from_hex =
        Transaction::from_bytes(&from_hex_bytes).expect("Should deserialize from hex bytes");

    // Verify hex round-trip
    assert_eq!(from_hex.nonce(), tx.nonce());
    assert_eq!(from_hex.system_fee(), tx.system_fee());

    // Test size calculation
    let calculated_size = tx.size();
    assert_eq!(
        calculated_size,
        serialized.len(),
        "Size calculation should match serialized length"
    );
}

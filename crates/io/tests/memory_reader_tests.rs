//! MemoryReader tests - matches C# UT_MemoryReader exactly
//!
//! This module contains tests that ensure MemoryReader functionality
//! matches the C# Neo.IO.MemoryReader exactly.

use neo_io::{IoResult, MemoryReader};
use std::io::Write;

/// Test ReadFixedString functionality (matches C# TestReadFixedString)
#[test]
fn test_read_fixed_string() {
    let data = b"AA\0"; // "AA" with null terminator
    let mut reader = MemoryReader::new(data);
    let result = reader.read_fixed_string(3).unwrap();
    assert_eq!("AA", result);
}

/// Test ReadVarString functionality (matches C# TestReadVarString)
#[test]
fn test_read_var_string() {
    // Create data with var string format: length (1 byte) + string
    let mut data = Vec::new();
    data.push(7); // Length of "AAAAAAA"
    data.extend_from_slice(b"AAAAAAA");

    let mut reader = MemoryReader::new(&data);
    let result = reader.read_var_string(10).unwrap();
    assert_eq!("AAAAAAA", result);
}

/// Test ReadSByte functionality (matches C# TestReadSByte)
#[test]
fn test_read_sbyte() {
    let values = vec![0i8, 1, -1, 5, -5, i8::MAX, i8::MIN];

    for v in values {
        let byte_array = vec![v as u8];
        let mut reader = MemoryReader::new(&byte_array);
        let n = reader.read_sbyte().unwrap();
        assert_eq!(v, n);
    }

    // Test overflow cases
    let values2 = vec![(i32::MAX as i64) + 1, (i32::MIN as i64) - 1];
    for v in values2 {
        let byte_array = vec![v as u8];
        let mut reader = MemoryReader::new(&byte_array);
        let n = reader.read_sbyte().unwrap();
        assert_eq!(v as i8, n);
    }
}

/// Test ReadInt32 functionality (matches C# TestReadInt32)
#[test]
fn test_read_int32() {
    let values = vec![0, 1, -1, 5, -5, i32::MAX, i32::MIN];

    for v in values {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_int32().unwrap();
        assert_eq!(v, n);
    }

    // Test overflow cases
    let values2 = vec![(i32::MAX as i64) + 1, (i32::MIN as i64) - 1];
    for v in values2 {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_int32().unwrap();
        assert_eq!(v as i32, n);
    }
}

/// Test ReadUInt64 functionality (matches C# TestReadUInt64)
#[test]
fn test_read_uint64() {
    let values = vec![0u64, 1, 5, u64::MAX, u64::MIN];

    for v in values {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_uint64().unwrap();
        assert_eq!(v, n);
    }

    // Test signed to unsigned conversion
    let values2 = vec![i64::MIN, -1, i64::MAX];
    for v in values2 {
        let bytes = v.to_le_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_uint64().unwrap();
        assert_eq!(v as u64, n);
    }
}

/// Test ReadInt16BigEndian functionality (matches C# TestReadInt16BigEndian)
#[test]
fn test_read_int16_big_endian() {
    let values = vec![i16::MIN, -1, 0, 1, 12345, i16::MAX];

    for v in values {
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_int16_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadUInt16BigEndian functionality (matches C# TestReadUInt16BigEndian)
#[test]
fn test_read_uint16_big_endian() {
    let values = vec![u16::MIN, 0, 1, 12345, u16::MAX];

    for v in values {
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_uint16_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadInt32BigEndian functionality (matches C# TestReadInt32BigEndian)
#[test]
fn test_read_int32_big_endian() {
    let values = vec![i32::MIN, -1, 0, 1, 12345, i32::MAX];

    for v in values {
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_int32_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadUInt32BigEndian functionality (matches C# TestReadUInt32BigEndian)
#[test]
fn test_read_uint32_big_endian() {
    let values = vec![u32::MIN, 0, 1, 12345, u32::MAX];

    for v in values {
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_uint32_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadInt64BigEndian functionality (matches C# TestReadInt64BigEndian)
#[test]
fn test_read_int64_big_endian() {
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
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_int64_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadUInt64BigEndian functionality (matches C# TestReadUInt64BigEndian)
#[test]
fn test_read_uint64_big_endian() {
    let values = vec![u64::MIN, 0, 1, 12345, u64::MAX];

    for v in values {
        let bytes = v.to_be_bytes();
        let mut reader = MemoryReader::new(&bytes);
        let n = reader.read_uint64_big_endian().unwrap();
        assert_eq!(v, n);
    }
}

/// Test ReadVarInt functionality (matches C# TestReadVarInt)
#[test]
fn test_read_var_int() {
    // Test single byte
    let data = vec![0x42];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x42, reader.read_var_int(u64::MAX).unwrap());

    // Test 2-byte value
    let data = vec![0xfd, 0x34, 0x12];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x1234, reader.read_var_int(u64::MAX).unwrap());

    // Test 4-byte value
    let data = vec![0xfe, 0x78, 0x56, 0x34, 0x12];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x12345678, reader.read_var_int(u64::MAX).unwrap());

    // Test 8-byte value
    let data = vec![0xff, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x12345678, reader.read_var_int(u64::MAX).unwrap());
}

/// Test ReadVarString functionality (matches C# TestReadVarString)
#[test]
fn test_read_var_string_basic() {
    let data = vec![0x05, b'h', b'e', b'l', b'l', b'o'];
    let mut reader = MemoryReader::new(&data);
    assert_eq!("hello", reader.read_var_string(1000).unwrap());
}

/// Test Position tracking (matches C# behavior)
#[test]
fn test_position() {
    let data = vec![0x01, 0x02, 0x03, 0x04];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0, reader.position());
    reader.read_byte().unwrap();
    assert_eq!(1, reader.position());
    reader.read_byte().unwrap();
    assert_eq!(2, reader.position());
}

/// Test Peek functionality (matches C# behavior)
#[test]
fn test_peek() {
    let data = vec![0x42, 0x43];
    let reader = MemoryReader::new(&data);
    assert_eq!(0x42, reader.peek().unwrap());
    assert_eq!(0, reader.position()); // Position should not change
}

/// Test error handling (matches C# behavior)
#[test]
fn test_ensure_position_error() {
    let data = vec![0x01];
    let mut reader = MemoryReader::new(&data);
    reader.read_byte().unwrap(); // Consume the only byte
    assert!(reader.read_byte().is_err()); // Should fail
}

/// Test ReadBoolean functionality (matches C# behavior)
#[test]
fn test_read_boolean() {
    let data = vec![0x00, 0x01, 0x02];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(false, reader.read_boolean().unwrap());
    assert_eq!(true, reader.read_boolean().unwrap());
    assert!(reader.read_boolean().is_err()); // Invalid boolean value
}

/// Test ReadByte functionality (matches C# behavior)
#[test]
fn test_read_byte() {
    let data = vec![0x42];
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x42, reader.read_byte().unwrap());
}

/// Test ReadUInt16 functionality (matches C# behavior)
#[test]
fn test_read_uint16() {
    let data = vec![0x78, 0x56]; // Little-endian 0x5678
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x5678, reader.read_uint16().unwrap());
}

/// Test ReadUInt32 functionality (matches C# behavior)
#[test]
fn test_read_uint32() {
    let data = vec![0x78, 0x56, 0x34, 0x12]; // Little-endian 0x12345678
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x12345678, reader.read_uint32().unwrap());
}

/// Test ReadUInt64 functionality (matches C# behavior)
#[test]
fn test_read_uint64_basic() {
    let data = vec![0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00]; // Little-endian
    let mut reader = MemoryReader::new(&data);
    assert_eq!(0x12345678, reader.read_uint64().unwrap());
}

/// Test ReadMemory functionality (matches C# behavior)
#[test]
fn test_read_memory() {
    let data = vec![0x01, 0x02, 0x03, 0x04];
    let mut reader = MemoryReader::new(&data);
    let result = reader.read_memory(2).unwrap();
    assert_eq!(&[0x01, 0x02], result);
    assert_eq!(2, reader.position());
}

/// Test ReadVarMemory functionality (matches C# behavior)
#[test]
fn test_read_var_memory() {
    let mut data = Vec::new();
    data.push(2); // Length
    data.extend_from_slice(&[0x01, 0x02]);

    let mut reader = MemoryReader::new(&data);
    let result = reader.read_var_memory(10).unwrap();
    assert_eq!(&[0x01, 0x02], result);
}

/// Test ReadToEnd functionality (matches C# behavior)
#[test]
fn test_read_to_end() {
    let data = vec![0x01, 0x02, 0x03, 0x04];
    let mut reader = MemoryReader::new(&data);
    reader.read_byte().unwrap(); // Skip first byte
    let result = reader.read_to_end().unwrap();
    assert_eq!(&[0x02, 0x03, 0x04], result);
    assert_eq!(4, reader.position());
}

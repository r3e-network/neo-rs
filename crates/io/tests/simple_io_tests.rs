//! Simple IO tests - basic functionality that works
//!
//! This module contains basic tests for IO functionality that actually compiles and runs.

use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};

/// Test basic MemoryReader functionality
#[test]
fn test_memory_reader_basic() {
    let data = vec![0x01, 0x02, 0x03, 0x04];
    let mut reader = MemoryReader::new(&data);

    assert_eq!(0x01, reader.read_byte().unwrap());
    assert_eq!(0x02, reader.read_byte().unwrap());
    assert_eq!(0x03, reader.read_byte().unwrap());
    assert_eq!(0x04, reader.read_byte().unwrap());
}

/// Test basic BinaryWriter functionality
#[test]
fn test_binary_writer_basic() {
    let mut writer = BinaryWriter::new();
    writer.write_byte(0x01).unwrap();
    writer.write_byte(0x02).unwrap();
    writer.write_byte(0x03).unwrap();
    writer.write_byte(0x04).unwrap();

    let result = writer.to_bytes();
    assert_eq!(vec![0x01, 0x02, 0x03, 0x04], result);
}

/// Test basic serialization round-trip
#[test]
fn test_serialization_round_trip() {
    #[derive(Debug, Clone, PartialEq)]
    struct TestStruct {
        pub value: u32,
        pub data: Vec<u8>,
    }

    impl Serializable for TestStruct {
        fn size(&self) -> usize {
            4 + self.data.len()
        }

        fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
            writer.write_u32(self.value)?;
            writer.write_bytes(&self.data)?;
            Ok(())
        }

        fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
            let value = reader.read_uint32()?;
            let data = reader.read_bytes(4)?; // Read 4 bytes
            Ok(TestStruct { value, data })
        }
    }

    let original = TestStruct {
        value: 0x12345678,
        data: vec![0xAA, 0xBB, 0xCC, 0xDD],
    };

    // Serialize
    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();
    let serialized = writer.to_bytes();

    // Deserialize
    let mut reader = MemoryReader::new(&serialized);
    let deserialized = TestStruct::deserialize(&mut reader).unwrap();

    assert_eq!(original.value, deserialized.value);
    assert_eq!(original.data, deserialized.data);
}

/// Test var int serialization
#[test]
fn test_var_int() {
    let mut writer = BinaryWriter::new();
    writer.write_var_int(0x42).unwrap();
    let result = writer.to_bytes();

    let mut reader = MemoryReader::new(&result);
    let value = reader.read_var_int(u64::MAX).unwrap();
    assert_eq!(0x42, value);
}

/// Test var string serialization
#[test]
fn test_var_string() {
    let mut writer = BinaryWriter::new();
    writer.write_var_string("hello").unwrap();
    let result = writer.to_bytes();

    let mut reader = MemoryReader::new(&result);
    let value = reader.read_var_string(100).unwrap();
    assert_eq!("hello", value);
}

/// Test var bytes serialization
#[test]
fn test_var_bytes() {
    let data = vec![0xAA, 0xBB, 0xCC];
    let mut writer = BinaryWriter::new();
    writer.write_var_bytes(&data).unwrap();
    let result = writer.to_bytes();

    let mut reader = MemoryReader::new(&result);
    let value = reader.read_var_bytes(100).unwrap();
    assert_eq!(data, value);
}

/// Test integer serialization
#[test]
fn test_integers() {
    let mut writer = BinaryWriter::new();
    writer.write_u16(0x1234).unwrap();
    writer.write_u32(0x12345678).unwrap();
    writer.write_u64(0x123456789ABCDEF0).unwrap();
    let result = writer.to_bytes();

    let mut reader = MemoryReader::new(&result);
    assert_eq!(0x1234, reader.read_uint16().unwrap());
    assert_eq!(0x12345678, reader.read_uint32().unwrap());
    assert_eq!(0x123456789ABCDEF0, reader.read_uint64().unwrap());
}

/// Test boolean serialization
#[test]
fn test_boolean() {
    let mut writer = BinaryWriter::new();
    writer.write_bool(true).unwrap();
    writer.write_bool(false).unwrap();
    let result = writer.to_bytes();

    let mut reader = MemoryReader::new(&result);
    assert_eq!(true, reader.read_boolean().unwrap());
    assert_eq!(false, reader.read_boolean().unwrap());
}

/// Test position tracking
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

/// Test peek functionality
#[test]
fn test_peek() {
    let data = vec![0x42, 0x43];
    let reader = MemoryReader::new(&data);
    assert_eq!(0x42, reader.peek().unwrap());
    assert_eq!(0, reader.position()); // Position should not change
}

/// Test error handling
#[test]
fn test_error_handling() {
    let data = vec![0x01];
    let mut reader = MemoryReader::new(&data);
    reader.read_byte().unwrap(); // Consume the only byte
    assert!(reader.read_byte().is_err()); // Should fail
}

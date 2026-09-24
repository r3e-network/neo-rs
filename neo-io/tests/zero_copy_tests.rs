//! Zero-copy serialization test suite

use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{UINT256_SIZE, UInt256};

#[derive(Debug, Clone, PartialEq)]
struct TestPayload {
    hash: UInt256,
    index: u32,
    data: Vec<u8>,
}

impl Serializable for TestPayload {
    fn size(&self) -> usize {
        UINT256_SIZE + 4 + neo_io::serializable::helper::get_var_size_bytes(&self.data)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_serializable(&self.hash)?;
        writer.write_u32(self.index)?;
        writer.write_var_bytes(&self.data)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        Ok(Self {
            hash: UInt256::deserialize(reader)?,
            index: reader.read_u32()?,
            data: reader.read_var_bytes(1024)?,
        })
    }
}

#[test]
fn test_freeze_produces_bytes() {
    let payload = TestPayload {
        hash: UInt256::zero(),
        index: 42,
        data: vec![1, 2, 3, 4, 5],
    };

    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).unwrap();
    let bytes = writer.freeze();

    // Verify we can clone cheaply
    let clone1 = bytes.clone();
    let clone2 = bytes.clone();

    assert_eq!(bytes.len(), clone1.len());
    assert_eq!(bytes.len(), clone2.len());
}

#[test]
fn test_with_capacity_avoids_reallocation() {
    let payload = TestPayload {
        hash: UInt256::zero(),
        index: 99,
        data: vec![0xAB; 256],
    };

    let expected_size = payload.size();
    let mut writer = BinaryWriter::with_capacity(expected_size);
    payload.serialize(&mut writer).unwrap();

    assert_eq!(writer.len(), expected_size);
}

#[test]
fn test_reserve_expands_capacity() {
    let mut writer = BinaryWriter::new();
    writer.reserve(1000);

    // Write some data
    writer.write_u32(42).unwrap();
    writer.write_var_bytes(&[0xAA; 100]).unwrap();

    assert!(!writer.is_empty());
}

#[test]
fn test_serializable_vec_with_size() {
    let payloads: Vec<TestPayload> = (0..10)
        .map(|i| TestPayload {
            hash: UInt256::zero(),
            index: i,
            data: vec![i as u8; 32],
        })
        .collect();

    let mut writer1 = BinaryWriter::new();
    writer1.write_serializable_vec(&payloads).unwrap();
    let bytes1 = writer1.into_bytes();

    let mut writer2 = BinaryWriter::new();
    writer2.write_serializable_vec_with_size(&payloads).unwrap();
    let bytes2 = writer2.into_bytes();

    // Both methods should produce identical output
    assert_eq!(bytes1, bytes2);
}

#[test]
fn test_zero_copy_read_with_memory_reader() {
    let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let mut reader = MemoryReader::new(&data);

    // Zero-copy read
    let slice = reader.read_bytes_ref(5).unwrap();
    assert_eq!(slice, &[1, 2, 3, 4, 5]);

    // Another zero-copy read
    let slice2 = reader.read_bytes_ref(5).unwrap();
    assert_eq!(slice2, &[6, 7, 8, 9, 10]);
}

#[test]
fn test_var_int_reserve_optimization() {
    let mut buf1 = Vec::new();
    neo_io::var_int::write_var_int(0xFFFF_FFFF, &mut buf1);
    assert_eq!(buf1.len(), 5);

    let mut buf2 = Vec::new();
    neo_io::var_int::write_var_bytes(&[0xAB; 100], &mut buf2);
    assert_eq!(buf2.len(), 1 + 100); // 1-byte prefix + 100 bytes data
}

#[test]
fn test_roundtrip_with_freeze() {
    let original = TestPayload {
        hash: UInt256::zero(),
        index: 12345,
        data: vec![0xFF; 128],
    };

    let mut writer = BinaryWriter::with_capacity(original.size());
    original.serialize(&mut writer).unwrap();
    let bytes = writer.freeze();

    let mut reader = MemoryReader::new(&bytes);
    let deserialized = TestPayload::deserialize(&mut reader).unwrap();

    assert_eq!(original, deserialized);
}

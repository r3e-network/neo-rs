use super::*;

#[test]
fn test_uint160_serialization() {
    let original = UInt160::from([1u8; 20]);
    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();

    let data = writer.to_bytes();
    assert_eq!(data.len(), UINT160_SIZE);

    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt160::deserialize(&mut reader).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_uint256_serialization() {
    let original = UInt256::from([2u8; 32]);
    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();

    let data = writer.to_bytes();
    assert_eq!(data.len(), UINT256_SIZE);

    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt256::deserialize(&mut reader).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_uint160_zero() {
    let zero = UInt160::zero();
    let mut writer = BinaryWriter::new();
    zero.serialize(&mut writer).unwrap();

    let data = writer.to_bytes();
    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt160::deserialize(&mut reader).unwrap();
    assert_eq!(zero, deserialized);
}

#[test]
fn test_uint256_zero() {
    let zero = UInt256::zero();
    let mut writer = BinaryWriter::new();
    zero.serialize(&mut writer).unwrap();

    let data = writer.to_bytes();
    let mut reader = MemoryReader::new(&data);
    let deserialized = UInt256::deserialize(&mut reader).unwrap();
    assert_eq!(zero, deserialized);
}

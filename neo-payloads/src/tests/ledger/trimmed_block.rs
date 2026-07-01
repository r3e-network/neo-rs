use super::*;
use neo_primitives::UInt160;
use num_bigint::BigInt;

/// Builds a header with distinctive, range-stressing field values. The nonce
/// is `u64::MAX` (well above `i64::MAX`) to guard the unsigned projection.
fn sample_header() -> Header {
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::from_bytes(&[0xA1u8; 32]).unwrap());
    header.set_merkle_root(UInt256::from_bytes(&[0xB2u8; 32]).unwrap());
    header.set_timestamp(0x0123_4567_89AB_CDEF);
    header.set_nonce(u64::MAX);
    header.set_index(123_456);
    header.set_primary_index(3);
    header.set_next_consensus(UInt160::from_bytes(&[0xC3u8; 20]).unwrap());
    header
}

fn sample_hashes() -> Vec<UInt256> {
    vec![
        UInt256::from_bytes(&[0x01u8; 32]).unwrap(),
        UInt256::from_bytes(&[0x02u8; 32]).unwrap(),
    ]
}

#[test]
fn serialize_deserialize_round_trips() {
    let original = TrimmedBlock::new(sample_header(), sample_hashes());

    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).unwrap();
    let bytes = writer.into_bytes();

    // size() must match the number of bytes actually written.
    assert_eq!(original.size(), bytes.len());

    let mut reader = MemoryReader::new(&bytes);
    let decoded = TrimmedBlock::deserialize(&mut reader).unwrap();

    // Header has no PartialEq (interior-mutable hash cache), so compare the
    // observable fields plus the computed hash.
    assert_eq!(decoded.header.version(), 0);
    assert_eq!(decoded.header.timestamp(), 0x0123_4567_89AB_CDEF);
    assert_eq!(decoded.header.nonce(), u64::MAX);
    assert_eq!(decoded.header.index(), 123_456);
    assert_eq!(decoded.header.primary_index(), 3);
    assert_eq!(decoded.header.hash(), original.header.hash());
    assert_eq!(decoded.hashes, original.hashes);
}

#[test]
fn deserialize_rejects_more_than_ushort_max_hashes() {
    // A length prefix above ushort.MaxValue (65535) must be rejected, exactly
    // like C# `ReadSerializableArray<UInt256>(ushort.MaxValue)`.
    let mut writer = BinaryWriter::new();
    <Header as Serializable>::serialize(&sample_header(), &mut writer).unwrap();
    writer.write_var_int(0x1_0000).unwrap(); // 65536
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    assert!(TrimmedBlock::deserialize(&mut reader).is_err());
}

#[test]
fn interoperable_to_stack_value_matches_inherent() {
    let header = sample_header();
    let hashes = sample_hashes();
    let block = TrimmedBlock::new(header, hashes);

    let trait_value = Interoperable::to_stack_value(&block).unwrap();
    let inherent_value = block.to_stack_value();
    assert_eq!(trait_value, inherent_value);
}

#[test]
fn to_stack_value_matches_csharp_layout() {
    let header = sample_header();
    let hashes = sample_hashes();
    let block = TrimmedBlock::new(header, hashes);

    let neo_vm_rs::StackValue::Array(fields) = block.to_stack_value() else {
        panic!("TrimmedBlock projects to an Array");
    };
    assert_eq!(fields.len(), 10, "C# ToStackItem produces a 10-field Array");

    assert_eq!(
        fields[0],
        neo_vm_rs::StackValue::ByteString(block.header.hash().to_bytes())
    );
    assert_eq!(fields[1], neo_vm_rs::StackValue::Integer(0));
    assert_eq!(
        fields[2],
        neo_vm_rs::StackValue::ByteString(block.header.prev_hash().to_bytes())
    );
    assert_eq!(
        fields[3],
        neo_vm_rs::StackValue::ByteString(block.header.merkle_root().to_bytes())
    );
    assert_eq!(
        fields[4],
        neo_vm_rs::StackValue::BigInteger(
            BigInt::from(0x0123_4567_89AB_CDEFu64).to_signed_bytes_le()
        )
    );
    assert_eq!(
        fields[5],
        neo_vm_rs::StackValue::BigInteger(BigInt::from(u64::MAX).to_signed_bytes_le())
    );
    assert_eq!(fields[6], neo_vm_rs::StackValue::Integer(123_456));
    assert_eq!(fields[7], neo_vm_rs::StackValue::Integer(3));
    assert_eq!(
        fields[8],
        neo_vm_rs::StackValue::ByteString(block.header.next_consensus().to_bytes())
    );
    assert_eq!(fields[9], neo_vm_rs::StackValue::Integer(2));
}

#[test]
fn from_stack_value_is_unsupported() {
    let mut block = TrimmedBlock::new(sample_header(), sample_hashes());
    let probe = StackValue::Integer(0);
    assert!(Interoperable::from_stack_value(&mut block, probe).is_err());
}

#[test]
fn from_block_trims_transactions_to_hashes() {
    // An empty block trims to an empty hash list and preserves the header.
    let block = Block::from_parts(sample_header(), Vec::new());
    let trimmed = TrimmedBlock::from_block(&block).unwrap();
    assert!(trimmed.hashes.is_empty());
    assert_eq!(trimmed.index(), 123_456);
    assert_eq!(trimmed.hash(), block.header.hash());
}

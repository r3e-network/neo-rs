use super::*;
use neo_crypto::Crypto;
use neo_vm::OpCode;

fn serializable_header_with_version(version: u32) -> Header {
    let mut header = Header::new();
    header.set_version(version);
    header.set_prev_hash(UInt256::from_bytes(&[1; 32]).expect("prev hash"));
    header.set_merkle_root(UInt256::from_bytes(&[2; 32]).expect("merkle root"));
    header.set_timestamp(1_700_000_000_000);
    header.set_nonce(0x0102_0304_0506_0708);
    header.set_index(42);
    header.set_primary_index(1);
    header.set_next_consensus(UInt160::from_bytes(&[3; 20]).expect("next consensus"));
    header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    header
}

#[test]
fn deserialize_rejects_nonzero_version_like_csharp() {
    let header = serializable_header_with_version(1);
    let mut writer = BinaryWriter::new();
    <Header as Serializable>::serialize(&header, &mut writer).expect("serialize header");
    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);

    let err = <Header as Serializable>::deserialize(&mut reader)
        .expect_err("C# Header.DeserializeUnsigned rejects Version > 0");
    assert!(
        err.to_string().contains("Header version must be 0"),
        "unexpected error: {err}"
    );
}

#[test]
fn header_hash_is_single_sha256_of_unsigned_data() {
    let header = serializable_header_with_version(0);
    let unsigned = header.try_get_hash_data().expect("header hash data");
    let first_digest = Crypto::sha256(&unsigned);
    let second_digest = Crypto::sha256(&first_digest);
    let expected_single = UInt256::from(first_digest);

    assert_eq!(header.try_hash().expect("header hash"), expected_single);
    assert_eq!(
        <Header as neo_primitives::SerializablePayload>::hash(&header),
        expected_single
    );
    assert_ne!(
        header.try_hash().expect("header hash"),
        UInt256::from(second_digest),
        "C# Helper.CalculateHash uses one SHA256 over Header.SerializeUnsigned"
    );
}

#[test]
fn header_to_bytes_round_trips_canonical_wire_format() {
    let header = serializable_header_with_version(0);
    let bytes = header.try_to_bytes().expect("serialize header");
    let mut writer = BinaryWriter::new();
    <Header as Serializable>::serialize(&header, &mut writer).expect("serialize header");

    assert_eq!(bytes, writer.into_bytes());

    let decoded = Header::from_bytes(&bytes).expect("decode header");
    assert_eq!(
        decoded.try_to_bytes().expect("serialize decoded header"),
        bytes
    );
    assert_eq!(decoded.index(), header.index());
    assert_eq!(decoded.prev_hash(), header.prev_hash());
    assert_eq!(decoded.merkle_root(), header.merkle_root());
}

#[test]
fn header_from_bytes_rejects_trailing_data() {
    let mut bytes = serializable_header_with_version(0)
        .try_to_bytes()
        .expect("serialize header");
    bytes.push(0);

    assert!(Header::from_bytes(&bytes).is_err());
}

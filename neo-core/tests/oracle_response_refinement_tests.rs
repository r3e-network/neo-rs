//! Oracle response wire-format regressions, not extracted Coq proofs.
//! Request lifecycle, authorization and callbacks are exercised separately in
//! oracle_contract_tests.rs; this suite does not claim timeout or nonce proofs.

use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::{
    oracle_response::{MAX_RESULT_SIZE, OracleResponse},
    oracle_response_code::OracleResponseCode,
};

fn encode(response: &OracleResponse) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    response
        .serialize_without_type(&mut writer)
        .expect("encode response");
    let bytes = writer.into_bytes();
    assert_eq!(bytes.len(), response.size());
    bytes
}

fn roundtrip(response: OracleResponse) {
    let bytes = encode(&response);
    let mut reader = MemoryReader::new(&bytes);
    let decoded =
        <OracleResponse as Serializable>::deserialize(&mut reader).expect("decode response");
    assert_eq!(decoded, response);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn success_roundtrip() {
    roundtrip(OracleResponse::new(
        12345,
        OracleResponseCode::Success,
        b"Hello".to_vec(),
    ));
}

#[test]
fn identifier_boundaries() {
    for id in [0, 1, u32::MAX as u64, u64::MAX] {
        roundtrip(OracleResponse::new(
            id,
            OracleResponseCode::Success,
            Vec::new(),
        ));
    }
}

#[test]
fn identifier_wire_order() {
    let id = 0x0102030405060708;
    let bytes = encode(&OracleResponse::new(
        id,
        OracleResponseCode::Success,
        Vec::new(),
    ));
    assert_eq!(&bytes[..8], &id.to_le_bytes());
    assert_eq!(&bytes[8..], &[0, 0]);
}

#[test]
fn result_varint_boundaries() {
    for length in [0, 1, 252, 253, 254, MAX_RESULT_SIZE] {
        roundtrip(OracleResponse::new(
            1,
            OracleResponseCode::Success,
            vec![42; length],
        ));
    }
}

#[test]
fn oversize_result_rejected_on_encode() {
    let response =
        OracleResponse::new(1, OracleResponseCode::Success, vec![0; MAX_RESULT_SIZE + 1]);
    assert!(
        response
            .serialize_without_type(&mut BinaryWriter::new())
            .is_err()
    );
}

#[test]
fn oversize_result_rejected_on_decode() {
    let mut writer = BinaryWriter::new();
    writer.write_u64(1).unwrap();
    writer.write_u8(0).unwrap();
    writer
        .write_var_bytes(&vec![0; MAX_RESULT_SIZE + 1])
        .unwrap();
    let bytes = writer.into_bytes();
    assert!(<OracleResponse as Serializable>::deserialize(&mut MemoryReader::new(&bytes)).is_err());
}

#[test]
fn every_response_code_matches_protocol_table() {
    let valid = [0x00u8, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1a, 0x1c, 0x1f, 0xff];
    for byte in 0..=u8::MAX {
        let mut bytes = vec![0; 8];
        bytes.extend_from_slice(&[byte, 0]);
        let result = <OracleResponse as Serializable>::deserialize(&mut MemoryReader::new(&bytes));
        assert_eq!(result.is_ok(), valid.contains(&byte), "code {byte:#04x}");
        if let Ok(response) = result {
            assert_eq!(response.code.to_byte(), byte);
            roundtrip(response);
        }
    }
}

#[test]
fn non_success_payload_is_rejected() {
    for byte in [0x10, 0x12, 0x14, 0x16, 0x18, 0x1a, 0x1c, 0x1f, 0xff] {
        let bytes = encode(&OracleResponse::new(
            9,
            OracleResponseCode::from_byte(byte).unwrap(),
            vec![1],
        ));
        assert!(
            <OracleResponse as Serializable>::deserialize(&mut MemoryReader::new(&bytes)).is_err()
        );
    }
}

#[test]
fn truncated_response_rejected() {
    let bytes = encode(&OracleResponse::new(
        42,
        OracleResponseCode::Success,
        vec![1, 2, 3],
    ));
    for end in 0..bytes.len() {
        assert!(
            <OracleResponse as Serializable>::deserialize(&mut MemoryReader::new(&bytes[..end]))
                .is_err(),
            "truncation at {end}"
        );
    }
}

#[test]
fn concatenated_responses_decode_independently() {
    let first = OracleResponse::new(1, OracleResponseCode::Success, vec![42]);
    let second = OracleResponse::new(2, OracleResponseCode::Error, Vec::new());
    let mut bytes = encode(&first);
    bytes.extend(encode(&second));
    let mut reader = MemoryReader::new(&bytes);
    assert_eq!(
        <OracleResponse as Serializable>::deserialize(&mut reader).unwrap(),
        first
    );
    assert_eq!(
        <OracleResponse as Serializable>::deserialize(&mut reader).unwrap(),
        second
    );
    assert_eq!(reader.remaining(), 0);
}

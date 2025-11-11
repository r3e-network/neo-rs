use alloc::{string::String, vec::Vec};

use super::{read_varint, write_varint, NeoDecode, NeoEncode, SliceReader};

#[test]
fn varint_roundtrip() {
    let numbers = [
        0u64,
        252,
        253,
        65_535,
        65_536,
        4_294_967_295,
        4_294_967_296,
        u64::MAX,
    ];

    for value in numbers {
        let mut buf = Vec::new();
        write_varint(&mut buf, value);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = read_varint(&mut reader).unwrap();
        assert_eq!(value, decoded);
    }
}

#[test]
fn bool_encoding() {
    let mut buf = Vec::new();
    true.neo_encode(&mut buf);
    false.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    assert!(bool::neo_decode(&mut reader).unwrap());
    assert!(!bool::neo_decode(&mut reader).unwrap());
}

#[test]
fn string_roundtrip() {
    let message = "neo-n3-rust";
    let mut buf = Vec::new();
    message.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = String::neo_decode(&mut reader).unwrap();
    assert_eq!(message, decoded);
}

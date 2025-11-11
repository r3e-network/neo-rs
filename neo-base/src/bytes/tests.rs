use alloc::vec::Vec;

use super::Bytes;
use crate::encoding::{NeoDecode, NeoEncode, SliceReader};

#[test]
fn bytes_roundtrip() {
    let original = Bytes::from(b"neo-n3".as_slice());
    let mut buf = Vec::new();
    original.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = Bytes::neo_decode(&mut reader).unwrap();
    assert_eq!(original, decoded);
}

#[cfg(feature = "std")]
#[test]
fn serde_base64() {
    let bytes = Bytes::from(vec![1u8, 2, 3, 4]);
    let encoded = serde_json::to_string(&bytes).unwrap();
    assert_eq!(encoded, "\"AQIDBA==\"");
    let decoded: Bytes = serde_json::from_str(&encoded).unwrap();
    assert_eq!(bytes, decoded);
}

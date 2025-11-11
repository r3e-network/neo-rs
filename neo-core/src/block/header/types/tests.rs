use super::Header;
use crate::{
    h160::H160,
    h256::H256,
    script::Script,
    tx::{Tx, Witness},
};
use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};

fn sample_header() -> Header {
    Header::new(
        0,
        H256::default(),
        H256::default(),
        1,
        42,
        1,
        0,
        H160::default(),
        vec![Witness::new(
            Script::new(vec![0x51]),
            Script::new(vec![0xAC]),
        )],
    )
}

#[test]
fn header_binary_roundtrip() {
    let header = sample_header();
    let mut buf = Vec::new();
    header.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let decoded = Header::neo_decode(&mut reader).expect("decode header");
    assert_eq!(decoded.version, header.version);
    assert_eq!(decoded.prev_hash, header.prev_hash);
}

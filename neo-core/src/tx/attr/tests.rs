use super::*;
use neo_base::{
    bytes::Bytes,
    encoding::{NeoDecode, NeoEncode, SliceReader},
};

#[test]
fn notary_assisted_roundtrip() {
    let attr = TxAttr::NotaryAssisted(NotaryAssisted { nkeys: 5 });
    let mut buf = Vec::new();
    attr.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let decoded = TxAttr::neo_decode(&mut reader).expect("decode notary");
    assert_eq!(decoded, attr);
}

#[test]
fn oracle_response_roundtrip() {
    let attr = TxAttr::OracleResponse(OracleResponse {
        id: 42,
        code: OracleCode::Timeout,
        result: Bytes::from(vec![1, 2, 3]),
    });
    let mut buf = Vec::new();
    attr.neo_encode(&mut buf);
    let mut reader = SliceReader::new(&buf);
    let decoded = TxAttr::neo_decode(&mut reader).expect("decode oracle");
    assert_eq!(decoded, attr);
}

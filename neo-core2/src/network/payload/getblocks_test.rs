use super::*;
use crate::testserdes;
use crate::crypto::hash;
use std::convert::TryInto;

#[test]
fn test_get_block_encode_decode() {
    let start = hash::sha256(b"a");

    let p = GetBlocks::new(start, 124);
    testserdes::encode_decode_binary(&p);

    // invalid count
    let p = GetBlocks::new(start, -2);
    let data = testserdes::encode_binary(&p).unwrap();
    assert!(testserdes::decode_binary::<GetBlocks>(&data).is_err());

    // invalid count
    let p = GetBlocks::new(start, 0);
    let data = testserdes::encode_binary(&p).unwrap();
    assert!(testserdes::decode_binary::<GetBlocks>(&data).is_err());
}

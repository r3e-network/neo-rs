use std::time::{SystemTime, UNIX_EPOCH};
use rand::Rng;
use crate::core::transaction::Witness;
use crate::crypto::hash;
use crate::util::Uint160;
use crate::core::block::Header;
use crate::testserdes;
use assert_eq::assert_eq;

fn test_header_encode_decode(state_root_enabled: bool) {
    let header = Header {
        version: 0,
        prev_hash: hash::sha256(b"prevhash"),
        merkle_root: hash::sha256(b"merkleroot"),
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        index: 3445,
        next_consensus: Uint160::default(),
        script: Witness {
            invocation_script: vec![0x10],
            verification_script: vec![0x11],
        },
        state_root_enabled,
        prev_state_root: if state_root_enabled { Some(rand::thread_rng().gen()) } else { None },
    };

    let _ = header.hash();
    let mut header_decode = Header { state_root_enabled, ..Default::default() };
    testserdes::encode_decode_binary(&header, &mut header_decode);

    assert_eq!(header.version, header_decode.version, "expected both versions to be equal");
    assert_eq!(header.prev_hash, header_decode.prev_hash, "expected both prev hashes to be equal");
    assert_eq!(header.merkle_root, header_decode.merkle_root, "expected both merkle roots to be equal");
    assert_eq!(header.index, header_decode.index, "expected both indexes to be equal");
    assert_eq!(header.next_consensus, header_decode.next_consensus, "expected both next consensus fields to be equal");
    assert_eq!(header.script.invocation_script, header_decode.script.invocation_script, "expected equal invocation scripts");
    assert_eq!(header.script.verification_script, header_decode.script.verification_script, "expected equal verification scripts");
    assert_eq!(header.prev_state_root, header_decode.prev_state_root, "expected equal state roots");
}

#[test]
fn test_header_encode_decode_no_state_root() {
    test_header_encode_decode(false);
}

#[test]
fn test_header_encode_decode_with_state_root() {
    test_header_encode_decode(true);
}

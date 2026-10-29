//! The ledger `BlockHeader` and the P2P `Header` must encode one Neo N3 header
//! as the same bytes. The unsigned layout lives in `ledger::header_wire`; this
//! test is the check that witness framing did not drift apart.

use neo_core::ledger::BlockHeader;
use neo_core::neo_io::{MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::payloads::header::Header;
use neo_core::{UInt160, UInt256, Witness};

fn encode<T: SerializableExt>(value: &T) -> Vec<u8> {
    value.to_array().expect("header encodes")
}

fn decode<T: Serializable>(bytes: &[u8]) -> T {
    try_decode(bytes).expect("header decodes")
}

fn try_decode<T: Serializable>(bytes: &[u8]) -> neo_core::neo_io::IoResult<T> {
    let mut reader = MemoryReader::new(bytes);
    T::deserialize(&mut reader)
}

fn sample_witness() -> Witness {
    let mut witness = Witness::new();
    witness.invocation_script = vec![0x0c, 0x40];
    witness.verification_script = vec![0x41, 0x56, 0xe7, 0xb3, 0x27];
    witness
}

fn sample_p2p_header() -> Header {
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from_bytes(&[0x11; 32]).unwrap());
    header.set_merkle_root(UInt256::from_bytes(&[0x22; 32]).unwrap());
    header.set_timestamp(1_700_000_000_000);
    header.set_nonce(0x0102_0304_0506_0708);
    header.set_index(1_074_782);
    header.set_primary_index(3);
    header.set_next_consensus(UInt160::from_bytes(&[0x33; 20]).unwrap());
    header.witness = sample_witness();
    header
}

fn sample_block_header() -> BlockHeader {
    BlockHeader::new(
        0,
        UInt256::from_bytes(&[0x11; 32]).unwrap(),
        UInt256::from_bytes(&[0x22; 32]).unwrap(),
        1_700_000_000_000,
        0x0102_0304_0506_0708,
        1_074_782,
        3,
        UInt160::from_bytes(&[0x33; 20]).unwrap(),
        vec![sample_witness()],
    )
}

#[test]
fn p2p_header_and_block_header_encode_the_same_bytes() {
    let p2p = sample_p2p_header();
    let ledger = sample_block_header();

    let p2p_bytes = encode(&p2p);
    let ledger_bytes = encode(&ledger);
    assert_eq!(p2p_bytes, ledger_bytes);
    assert_eq!(p2p.size(), ledger.size());

    let from_p2p: BlockHeader = decode(&p2p_bytes);
    let from_ledger: Header = decode(&ledger_bytes);
    assert_eq!(encode(&from_p2p), ledger_bytes);
    assert_eq!(encode(&from_ledger), p2p_bytes);
    assert_eq!(from_p2p.hash(), ledger.hash());
}

#[test]
fn either_type_rejects_a_second_witness() {
    let mut bytes = encode(&sample_block_header());
    // The witness count is the first byte after the 4+32+32+8+8+4+1+20 unsigned prefix.
    let count_at = 4 + 32 + 32 + 8 + 8 + 4 + 1 + 20;
    assert_eq!(bytes[count_at], 1);
    bytes[count_at] = 2;
    assert!(try_decode::<BlockHeader>(&bytes).is_err());
    assert!(try_decode::<Header>(&bytes).is_err());
}

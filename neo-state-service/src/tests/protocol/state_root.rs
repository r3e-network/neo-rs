use super::*;

#[test]
fn state_root_hash_is_stable() {
    let mut a = StateRoot::new_current(42, UInt256::from([0xAAu8; 32]));
    let mut b = StateRoot::new_current(42, UInt256::from([0xAAu8; 32]));
    assert_eq!(a.hash(), b.hash());
}

#[test]
fn different_roots_yield_different_hashes() {
    let mut a = StateRoot::new_current(1, UInt256::from([0x01u8; 32]));
    let mut b = StateRoot::new_current(2, UInt256::from([0x01u8; 32]));
    assert_ne!(a.hash(), b.hash());
}

#[test]
fn unsigned_bytes_round_trip() {
    let sr = StateRoot::new_current(7, UInt256::from([0x33u8; 32]));
    let bytes = sr.unsigned_bytes();
    assert_eq!(bytes.len(), 1 + 4 + 32);
    assert_eq!(bytes[0], CURRENT_VERSION);
}

#[test]
fn full_serialization_round_trips_with_no_witness() {
    let sr = StateRoot::new_current(7, UInt256::from([0x33u8; 32]));
    let bytes = sr.to_array();
    // unsigned (1+4+32) + a 0-element witness var-array (var-int 0 = one byte).
    assert_eq!(bytes.len(), 1 + 4 + 32 + 1);
    assert_eq!(*bytes.last().unwrap(), 0u8, "0-witness var-array");

    let mut reader = neo_io::MemoryReader::new(&bytes);
    let parsed = StateRoot::deserialize(&mut reader).unwrap();
    assert_eq!(parsed.version, CURRENT_VERSION);
    assert_eq!(parsed.index, 7);
    assert_eq!(parsed.root_hash, UInt256::from([0x33u8; 32]));
    assert!(parsed.witness().is_none());
}

#[test]
fn signed_state_root_round_trips_witness() {
    let witness = neo_payloads::Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5]);
    let sr = StateRoot::new_current(9, UInt256::from([0x44u8; 32])).with_witness(witness);
    let bytes = sr.to_array();

    let mut reader = neo_io::MemoryReader::new(&bytes);
    let parsed = StateRoot::deserialize(&mut reader).unwrap();
    assert_eq!(parsed.index, 9);
    let pw = parsed.witness().expect("witness present");
    assert_eq!(pw.invocation_script, vec![1, 2, 3]);
    assert_eq!(pw.verification_script, vec![4, 5]);
}

#[test]
fn get_sign_data_is_network_le_then_hash() {
    let mut sr = StateRoot::new_current(5, UInt256::from([0x55u8; 32]));
    let hash = sr.hash();
    let network = 0x4E45_4F4Eu32;
    let data = sr.get_sign_data(network);
    assert_eq!(data.len(), 4 + 32);
    assert_eq!(&data[0..4], &network.to_le_bytes());
    assert_eq!(&data[4..], &hash.to_bytes());
}

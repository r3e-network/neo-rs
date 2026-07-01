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

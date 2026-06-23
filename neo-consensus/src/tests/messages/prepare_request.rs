use super::*;

#[test]
fn test_prepare_request_new() {
    let msg = PrepareRequestMessage::new(
        100,
        0,
        0,
        0,
        UInt256::zero(),
        1234567890,
        42,
        vec![UInt256::zero()],
    );

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 0);
    assert_eq!(msg.version, 0);
    assert_eq!(msg.prev_hash, UInt256::zero());
    assert_eq!(msg.timestamp, 1234567890);
    assert_eq!(msg.nonce, 42);
    assert_eq!(msg.transaction_hashes.len(), 1);
}

#[test]
fn test_prepare_request_serialize() {
    let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, vec![]);
    let data = msg.serialize();

    // version (4) + prev_hash (32) + timestamp (8) + nonce (8) + tx count (1)
    assert_eq!(data.len(), 53);
}

#[test]
fn test_prepare_request_wire_format_bytes() {
    let prev_hash = UInt256::from([0xAAu8; 32]);
    let tx1 = UInt256::from([0x01u8; 32]);
    let tx2 = UInt256::from([0x02u8; 32]);
    let timestamp = 0x0A0B_0C0D_0102_0304u64;
    let nonce = 0x1122_3344_5566_7788u64;

    let msg = PrepareRequestMessage::new(100, 0, 0, 0, prev_hash, timestamp, nonce, vec![tx1, tx2]);
    let data = msg.serialize();

    let mut expected = Vec::new();
    expected.extend_from_slice(&0u32.to_le_bytes());
    expected.extend_from_slice(&prev_hash.to_array());
    expected.extend_from_slice(&timestamp.to_le_bytes());
    expected.extend_from_slice(&nonce.to_le_bytes());
    expected.push(0x02); // varint count
    expected.extend_from_slice(&tx1.to_array());
    expected.extend_from_slice(&tx2.to_array());

    assert_eq!(data, expected);
}

#[test]
fn test_prepare_request_validate() {
    let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, vec![]);

    assert!(msg.validate(0, 512).is_ok());
    assert!(msg.validate(1, 512).is_err());
}

#[test]
fn test_prepare_request_validate_rejects_too_many_transactions() {
    let tx_hashes = vec![UInt256::from([0x01; 32]), UInt256::from([0x02; 32])];
    let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, tx_hashes);

    assert!(msg.validate(0, 1).is_err());
    assert!(msg.validate(0, 2).is_ok());
}

use super::*;

#[test]
fn test_change_view_new() {
    let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 1);
    assert_eq!(msg.new_view_number().unwrap(), 1);
    assert_eq!(msg.reason, ChangeViewReason::Timeout);
}

#[test]
fn test_change_view_serialize() {
    let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
    let data = msg.serialize();

    // 8 bytes timestamp + 1 byte reason
    assert_eq!(data.len(), 9);
}

#[test]
fn test_change_view_wire_format_bytes() {
    let timestamp = 0x0102_0304_0506_0708u64;
    let msg = ChangeViewMessage::new(100, 7, 1, timestamp, ChangeViewReason::TxNotFound);
    let data = msg.serialize();

    let mut expected = Vec::new();
    expected.extend_from_slice(&timestamp.to_le_bytes());
    expected.push(ChangeViewReason::TxNotFound.to_byte());
    assert_eq!(data, expected);
}

#[test]
fn test_change_view_validate() {
    let valid = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
    assert!(valid.validate().is_ok());

    // Overflow case cannot be constructed as valid.
    let overflow = ChangeViewMessage::new(100, u8::MAX, 1, 1000, ChangeViewReason::Timeout);
    assert!(overflow.validate().is_err());
}

#[test]
fn test_change_view_serialize_deserialize_roundtrip() {
    let msg = ChangeViewMessage::new(100, 0, 1, 12345678, ChangeViewReason::TxNotFound);
    let data = msg.serialize();

    let parsed = ChangeViewMessage::deserialize(&data, 100, 0, 1).unwrap();

    assert_eq!(parsed.block_index, 100);
    assert_eq!(parsed.view_number, 0);
    assert_eq!(parsed.validator_index, 1);
    assert_eq!(parsed.new_view_number().unwrap(), 1);
    assert_eq!(parsed.timestamp, 12345678);
    assert_eq!(parsed.reason, ChangeViewReason::TxNotFound);
}

#[test]
fn test_change_view_deserialize_too_short() {
    let data = vec![0u8; 5]; // Too short
    let result = ChangeViewMessage::deserialize(&data, 100, 0, 1);
    assert!(result.is_err());
}

#[test]
fn test_change_view_tx_invalid_round_trips_rejected_hashes() {
    // C# v3.10.0 ChangeView serializes RejectedHashes (UInt256[]) ONLY for the
    // TxRejectedByPolicy/TxInvalid reasons: timestamp(8) + reason(1) + var-int
    // count + 32 bytes per hash.
    let hashes = vec![
        neo_primitives::UInt256::from_bytes(&[0x11u8; 32]).unwrap(),
        neo_primitives::UInt256::from_bytes(&[0x22u8; 32]).unwrap(),
    ];
    let msg = ChangeViewMessage::new(7, 2, 3, 0xDEAD_BEEF, ChangeViewReason::TxInvalid)
        .with_rejected_hashes(hashes.clone());
    let data = msg.serialize();
    assert_eq!(
        data.len(),
        8 + 1 + 1 + 2 * 32,
        "ts + reason + var-int(2) + 2 hashes"
    );

    let parsed = ChangeViewMessage::deserialize(&data, 7, 2, 3).unwrap();
    assert_eq!(parsed.reason, ChangeViewReason::TxInvalid);
    assert_eq!(parsed.rejected_hashes, hashes);
}

#[test]
fn test_change_view_non_tx_reason_omits_rejected_hashes() {
    // A non-TxInvalid reason must NOT serialize RejectedHashes even if set
    // (C# only writes them for TxRejectedByPolicy/TxInvalid) — wire compat.
    let msg = ChangeViewMessage::new(7, 2, 3, 1000, ChangeViewReason::Timeout)
        .with_rejected_hashes(vec![
            neo_primitives::UInt256::from_bytes(&[0x33u8; 32]).unwrap(),
        ]);
    let data = msg.serialize();
    assert_eq!(data.len(), 9, "Timeout serializes only timestamp + reason");

    let parsed = ChangeViewMessage::deserialize(&data, 7, 2, 3).unwrap();
    assert!(parsed.rejected_hashes.is_empty());
}

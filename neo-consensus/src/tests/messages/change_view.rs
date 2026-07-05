use super::*;
use neo_primitives::UInt256;

#[tokio::test]
async fn test_change_view_new() {
    let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout, Vec::new());

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 1);
    assert_eq!(msg.new_view_number().unwrap(), 1);
    assert_eq!(msg.reason, ChangeViewReason::Timeout);
    assert!(msg.rejected_hashes.is_empty());
}

#[tokio::test]
async fn test_change_view_serialize() {
    let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout, Vec::new());
    let data = msg.serialize();

    // 8 bytes timestamp + 1 byte reason (no trailing array for Timeout)
    assert_eq!(data.len(), 9);
}

#[tokio::test]
async fn test_change_view_wire_format_bytes() {
    let timestamp = 0x0102_0304_0506_0708u64;
    let msg = ChangeViewMessage::new(
        100,
        7,
        1,
        timestamp,
        ChangeViewReason::TxNotFound,
        Vec::new(),
    );
    let data = msg.serialize();

    let mut expected = Vec::new();
    expected.extend_from_slice(&timestamp.to_le_bytes());
    expected.push(ChangeViewReason::TxNotFound.to_byte());
    assert_eq!(data, expected);
}

#[tokio::test]
async fn test_change_view_validate() {
    let valid = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout, Vec::new());
    assert!(valid.validate().is_ok());

    // Overflow case cannot be constructed as valid.
    let overflow =
        ChangeViewMessage::new(100, u8::MAX, 1, 1000, ChangeViewReason::Timeout, Vec::new());
    assert!(overflow.validate().is_err());
}

#[tokio::test]
async fn test_change_view_serialize_deserialize_roundtrip() {
    let msg = ChangeViewMessage::new(
        100,
        0,
        1,
        12345678,
        ChangeViewReason::TxNotFound,
        Vec::new(),
    );
    let data = msg.serialize();

    let parsed = ChangeViewMessage::deserialize(&data, 100, 0, 1).unwrap();

    assert_eq!(parsed.block_index, 100);
    assert_eq!(parsed.view_number, 0);
    assert_eq!(parsed.validator_index, 1);
    assert_eq!(parsed.new_view_number().unwrap(), 1);
    assert_eq!(parsed.timestamp, 12345678);
    assert_eq!(parsed.reason, ChangeViewReason::TxNotFound);
    assert!(parsed.rejected_hashes.is_empty());
}

#[tokio::test]
async fn test_change_view_deserialize_too_short() {
    let data = vec![0u8; 5]; // Too short
    let result = ChangeViewMessage::deserialize(&data, 100, 0, 1);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_change_view_non_rejection_reasons_have_no_trailing_array() {
    // C# `DBFTPlugin` ChangeView.Serialize (v3.10.0) writes ONLY Timestamp(8) +
    // Reason(1) for every reason EXCEPT TxRejectedByPolicy/TxInvalid, which append
    // a RejectedHashes UInt256[]. Verify the non-rejection reasons stay at 9 bytes
    // so the signed body matches C# peers exactly.
    for reason in [
        ChangeViewReason::Timeout,
        ChangeViewReason::ChangeAgreement,
        ChangeViewReason::TxNotFound,
        ChangeViewReason::BlockRejectedByPolicy,
    ] {
        let msg = ChangeViewMessage::new(7, 2, 3, 0xDEAD_BEEF, reason, Vec::new());
        let data = msg.serialize();
        assert_eq!(
            data.len(),
            9,
            "{reason:?} must serialize only timestamp+reason (no trailing array)"
        );
        assert_eq!(msg.size(), 9, "{reason:?} size must be 9");
        let parsed = ChangeViewMessage::deserialize(&data, 7, 2, 3).unwrap();
        assert_eq!(parsed.reason, reason);
        assert_eq!(parsed.timestamp, 0xDEAD_BEEF);
        assert!(parsed.rejected_hashes.is_empty());
    }
}

/// A `Timeout` ChangeView produces NO trailing array — exactly 9 bytes.
#[tokio::test]
async fn test_change_view_timeout_no_trailing_array() {
    let msg = ChangeViewMessage::new(1, 0, 0, 0x1122_3344_5566_7788, ChangeViewReason::Timeout, {
        // Even if hashes are (incorrectly) supplied, Timeout must NOT serialize them.
        vec![UInt256::from_bytes(&[0xAB; 32]).unwrap()]
    });
    let data = msg.serialize();
    assert_eq!(
        data.len(),
        9,
        "Timeout must emit only timestamp(8)+reason(1)"
    );
    assert_eq!(data[8], ChangeViewReason::Timeout.to_byte());
}

/// Byte-exact round-trip for a `TxRejectedByPolicy` ChangeView carrying 2 hashes.
///
/// Asserts the C# v3.10.0 wire layout: Timestamp(u64 LE) + Reason(0x03) +
/// var-int count(0x02) + 32 raw bytes per UInt256 (stored order), then
/// deserializes and asserts full equality.
#[tokio::test]
async fn test_change_view_rejected_hashes_byte_layout_roundtrip() {
    let timestamp = 0x0807_0605_0403_0201u64;
    let h0 = UInt256::from_bytes(&[0x11; 32]).unwrap();
    let h1 = UInt256::from_bytes(&[0x22; 32]).unwrap();

    let msg = ChangeViewMessage::new(
        42,
        3,
        5,
        timestamp,
        ChangeViewReason::TxRejectedByPolicy,
        vec![h0, h1],
    );
    let data = msg.serialize();

    // Build the exact expected bytes.
    let mut expected = Vec::new();
    expected.extend_from_slice(&timestamp.to_le_bytes()); // Timestamp: u64 LE
    expected.push(0x03); // Reason: TxRejectedByPolicy
    expected.push(0x02); // var-int count = 2
    expected.extend_from_slice(&h0.to_bytes()); // 32 raw bytes (stored order)
    expected.extend_from_slice(&h1.to_bytes()); // 32 raw bytes (stored order)

    assert_eq!(data, expected, "byte-exact C# v3.10.0 layout");
    // 8 + 1 + 1 (var-int) + 32 + 32 = 74 bytes.
    assert_eq!(data.len(), 74);
    assert_eq!(msg.size(), 74, "size() must match serialized length");

    // Round-trip back.
    let parsed = ChangeViewMessage::deserialize(&data, 42, 3, 5).unwrap();
    assert_eq!(parsed.timestamp, timestamp);
    assert_eq!(parsed.reason, ChangeViewReason::TxRejectedByPolicy);
    assert_eq!(parsed.rejected_hashes, vec![h0, h1]);
    assert_eq!(parsed.block_index, 42);
    assert_eq!(parsed.view_number, 3);
    assert_eq!(parsed.validator_index, 5);
}

/// `TxInvalid` (0x04) also carries the RejectedHashes array (empty here).
#[tokio::test]
async fn test_change_view_tx_invalid_empty_array_roundtrip() {
    let msg = ChangeViewMessage::new(1, 0, 0, 999, ChangeViewReason::TxInvalid, Vec::new());
    let data = msg.serialize();

    // Timestamp(8) + Reason(0x04) + var-int count(0x00) = 10 bytes.
    assert_eq!(data.len(), 10);
    assert_eq!(data[8], 0x04);
    assert_eq!(data[9], 0x00, "empty UInt256[] is var-int 0x00");
    assert_eq!(msg.size(), 10);

    let parsed = ChangeViewMessage::deserialize(&data, 1, 0, 0).unwrap();
    assert_eq!(parsed.reason, ChangeViewReason::TxInvalid);
    assert!(parsed.rejected_hashes.is_empty());
}

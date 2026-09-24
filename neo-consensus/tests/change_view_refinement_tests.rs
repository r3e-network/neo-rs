// Phase-1 Module #15 - ChangeView Message Refinement Tests
// Coq counterpart: formal/coqlib/change_view_message_refinement.v

use neo_consensus::{ChangeViewMessage, ChangeViewReason};

/// Test 1: view_number_bounds - view 0 is within valid range [0, 255]
#[test]
fn test_view_number_bounds_minimum() {
    // Corresponds to Coq lemma: view_number_bounds (view = 0 case)
    let msg = ChangeViewMessage::new(100, 0, 1, 1_000_000, ChangeViewReason::Timeout);

    assert!(msg.validate().is_ok());
    assert_eq!(msg.view_number, 0);
}

/// Test 2: view_number_bounds - view 254 is maximum safe value (< 255 ensures new_view exists)
#[test]
fn test_view_number_bounds_maximum_safe() {
    // Corresponds to Coq theorem: view_always_advances with view=254 -> new_view=255
    let msg = ChangeViewMessage::new(100, 254, 1, 1_000_000, ChangeViewReason::Timeout);

    assert!(msg.validate().is_ok());
    assert_eq!(msg.new_view_number().unwrap(), 255);
}

/// Test 3: view_number_bounds - view 255 causes overflow (invalid)
#[test]
fn test_view_number_bounds_overflow() {
    // Corresponds to Coq theorem: invalid_changeview_overflow_rejected
    let msg = ChangeViewMessage::new(100, 255, 1, 1_000_000, ChangeViewReason::Timeout);

    assert!(msg.validate().is_err());
    assert_eq!(
        msg.new_view_number().unwrap_err().to_string(),
        "Invalid block proposal: View number overflow"
    );
}

/// Test 4: validation_safety - Timeout reason validates successfully
#[test]
fn test_timeout_reason_validates() {
    // Corresponds to Coq example: valid_changeview_view0 with reason=Timeout (0x0)
    let msg = ChangeViewMessage::new(100, 0, 1, 1_000_000, ChangeViewReason::Timeout);

    assert!(msg.validate().is_ok());
    assert_eq!(msg.reason, ChangeViewReason::Timeout);
}

/// Test 5: validation_safety - All six reason variants validate correctly
#[test]
fn test_all_reason_variants_validate() {
    // Corresponds to Coq theorem: all_reasons_are_valid
    let reasons = vec![
        ChangeViewReason::Timeout,
        ChangeViewReason::ChangeAgreement,
        ChangeViewReason::TxNotFound,
        ChangeViewReason::TxRejectedByPolicy,
        ChangeViewReason::TxInvalid,
        ChangeViewReason::BlockRejectedByPolicy,
    ];

    for reason in reasons {
        let msg = ChangeViewMessage::new(100, 0, 1, 1_000_000, reason);
        assert!(
            msg.validate().is_ok(),
            "Reason {:?} should be valid",
            reason
        );
    }
}

/// Test 6: serialization_format - Correct length (9 bytes: 8 timestamp + 1 reason)
#[test]
fn test_serialization_length() {
    // Corresponds to Coq theorem: serialize_exact_length_for_valid_message (SERIALIZED_LENGTH = 9)
    let msg = ChangeViewMessage::new(100, 0, 1, 1_234_567, ChangeViewReason::TxNotFound);

    let serialized = msg.serialize();
    assert_eq!(
        serialized.len(),
        9,
        "Serialized ChangeView must be exactly 9 bytes"
    );
}

/// Test 7: serialization_format - Timestamp encoded as little-endian 8-byte integer
#[test]
fn test_serialization_timestamp_le_bytes() {
    // Verifies timestamp byte ordering matches Coq model's 8-byte component
    let timestamp = 0x0102_0304_0506_0708u64;
    let msg = ChangeViewMessage::new(100, 0, 1, timestamp, ChangeViewReason::Timeout);

    let serialized = msg.serialize();

    // Little-endian encoding: least significant byte first
    assert_eq!(serialized[0], 0x08);
    assert_eq!(serialized[1], 0x07);
    assert_eq!(serialized[2], 0x06);
    assert_eq!(serialized[3], 0x05);
    assert_eq!(serialized[4], 0x04);
    assert_eq!(serialized[5], 0x03);
    assert_eq!(serialized[6], 0x02);
    assert_eq!(serialized[7], 0x01);
    assert_eq!(serialized[8], ChangeViewReason::Timeout.to_byte());
}

/// Test 8: serialization_format - Reason byte at position 8
#[test]
fn test_serialization_reason_byte_position() {
    // Verifies reason occupies last byte as specified in Coq model
    let msg = ChangeViewMessage::new(100, 5, 3, 1_000_000, ChangeViewReason::TxInvalid);

    let serialized = msg.serialize();

    assert_eq!(serialized[8], ChangeViewReason::TxInvalid.to_byte());
    assert_eq!(serialized.len(), 9);
}

/// Test 9: view progression - view always increments by exactly 1
#[test]
fn test_view_progression_increment() {
    // Corresponds to Coq lemma: view_always_advances
    for view in [0u8, 1, 10, 50, 100, 200, 250, 254] {
        let msg = ChangeViewMessage::new(100, view, 1, 1_000_000, ChangeViewReason::Timeout);
        assert!(msg.validate().is_ok());
        assert_eq!(msg.new_view_number().unwrap(), view + 1);
    }
}

/// Test 10: edge cases - validator_index boundary conditions
#[test]
fn test_validator_index_boundaries() {
    // Validator index is u8, bounded [0, 255]
    let msg_min = ChangeViewMessage::new(100, 0, 0, 1_000_000, ChangeViewReason::Timeout);
    let msg_max = ChangeViewMessage::new(100, 0, 255, 1_000_000, ChangeViewReason::Timeout);

    assert!(msg_min.validate().is_ok());
    assert!(msg_max.validate().is_ok());
}

/// Test 11: edge cases - timestamp at boundaries
#[test]
fn test_timestamp_boundaries() {
    // Timestamp can be 0 or any u64 value
    let msg_zero = ChangeViewMessage::new(100, 0, 1, 0, ChangeViewReason::Timeout);
    let msg_large = ChangeViewMessage::new(100, 0, 1, u64::MAX, ChangeViewReason::Timeout);

    assert!(msg_zero.validate().is_ok());
    assert!(msg_large.validate().is_ok());
}

/// Test 12: safety property - validated messages enable view advancement
#[test]
fn test_validation_enables_view_advancement() {
    // Corresponds to Coq lemma: validation_preserves_safety

    // Valid message enables advancement
    let valid_msg = ChangeViewMessage::new(100, 5, 1, 1_000_000, ChangeViewReason::Timeout);
    assert!(valid_msg.validate().is_ok());
    assert_eq!(valid_msg.new_view_number().unwrap(), 6);

    // Invalid message blocks advancement
    let invalid_msg = ChangeViewMessage::new(100, 255, 1, 1_000_000, ChangeViewReason::Timeout);
    assert!(invalid_msg.validate().is_err());
}

/// Test 13: round-trip consistency - deserialize preserves validation properties
#[test]
fn test_deserialize_preserves_validation() {
    // Serialize and deserialize a valid message
    let original = ChangeViewMessage::new(100, 10, 5, 2_000_000, ChangeViewReason::TxNotFound);
    let serialized = original.serialize();

    let reconstructed = ChangeViewMessage::deserialize(&serialized, 100, 10, 5).unwrap();

    assert_eq!(original.block_index, reconstructed.block_index);
    assert_eq!(original.view_number, reconstructed.view_number);
    assert_eq!(original.validator_index, reconstructed.validator_index);
    assert_eq!(original.timestamp, reconstructed.timestamp);
    assert_eq!(original.reason, reconstructed.reason);

    // Reconstruction must preserve validity
    assert!(reconstructed.validate().is_ok());
}

/// Test 14: no regression property - views never decrease or repeat
#[test]
fn test_no_view_regression() {
    // If msg1 and msg2 have valid views, new_view(msg2) >= new_view(msg1) when msg2.view >= msg1.view

    let msg1 = ChangeViewMessage::new(100, 5, 1, 1_000_000, ChangeViewReason::Timeout);
    let msg2 = ChangeViewMessage::new(100, 10, 1, 2_000_000, ChangeViewReason::Timeout);

    assert!(msg1.validate().is_ok());
    assert!(msg2.validate().is_ok());

    let view1_new = msg1.new_view_number().unwrap();
    let view2_new = msg2.new_view_number().unwrap();

    assert!(view2_new >= view1_new);
    assert_eq!(view2_new, view1_new + 5); // Increment property
}

/// Test 15: reason bounds verification - all reasons map to 0-5 range
#[test]
fn test_reason_bounds_verification() {
    // Corresponds to Coq lemma: reason_in_bounds
    assert_eq!(ChangeViewReason::Timeout.to_byte(), 0x0);
    assert_eq!(ChangeViewReason::ChangeAgreement.to_byte(), 0x1);
    assert_eq!(ChangeViewReason::TxNotFound.to_byte(), 0x2);
    assert_eq!(ChangeViewReason::TxRejectedByPolicy.to_byte(), 0x3);
    assert_eq!(ChangeViewReason::TxInvalid.to_byte(), 0x4);
    assert_eq!(ChangeViewReason::BlockRejectedByPolicy.to_byte(), 0x5);

    // Verify all are within protocol bounds
    for reason in [
        ChangeViewReason::Timeout,
        ChangeViewReason::ChangeAgreement,
        ChangeViewReason::TxNotFound,
        ChangeViewReason::TxRejectedByPolicy,
        ChangeViewReason::TxInvalid,
        ChangeViewReason::BlockRejectedByPolicy,
    ] {
        assert!(reason.to_byte() <= 5, "{:?} should be <= 5", reason);
    }
}

/// Test 16: Coq integration - verify key values match formal model
#[test]
fn test_coq_integration_constant_values() {
    // Match Coq definitions from change_view_message_refinement.v
    assert_eq!(ChangeViewReason::Timeout.to_byte(), 0);
    assert_eq!(ChangeViewReason::ChangeAgreement.to_byte(), 1);
    assert_eq!(ChangeViewReason::TxNotFound.to_byte(), 2);
    assert_eq!(ChangeViewReason::TxRejectedByPolicy.to_byte(), 3);
    assert_eq!(ChangeViewReason::TxInvalid.to_byte(), 4);
    assert_eq!(ChangeViewReason::BlockRejectedByPolicy.to_byte(), 5);

    // Maximum view constant
    const MAX_VIEW: u8 = 255;
    assert_eq!(MAX_VIEW, 255);
    assert!(254 < MAX_VIEW); // 254 can advance to 255
}

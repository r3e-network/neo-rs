use super::*;

#[tokio::test]
async fn recovery_request_serializes_timestamp_only() {
    let msg = RecoveryRequestMessage::new(1, 0, 0, 1234);
    let bytes = msg.serialize();
    assert_eq!(bytes.len(), 8);
    assert_eq!(u64::from_le_bytes(bytes.try_into().unwrap()), 1234);
}

#[tokio::test]
async fn recovery_message_roundtrip_minimal_without_prepare_request() {
    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.preparation_hash = Some(UInt256::from([0xAB; 32]));
    msg.preparation_messages.push(PreparationPayloadCompact {
        validator_index: 0,
        invocation_script: vec![0x0C, 0x40, 0xAA],
    });
    msg.commit_messages.push(CommitPayloadCompact {
        view_number: 0,
        validator_index: 0,
        signature: vec![0x11; 64],
        invocation_script: vec![0x0C, 0x40, 0xBB],
    });

    let bytes = msg.serialize().unwrap();
    let parsed = RecoveryMessage::deserialize(&bytes, 100, 0, 1).unwrap();
    assert!(parsed.prepare_request_message.is_none());
    assert_eq!(parsed.preparation_hash, msg.preparation_hash);
    assert_eq!(parsed.preparation_messages.len(), 1);
    assert_eq!(parsed.commit_messages.len(), 1);
}

#[tokio::test]
async fn recovery_message_wire_format_bytes_without_prepare_request() {
    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.change_view_messages.push(ChangeViewPayloadCompact {
        validator_index: 2,
        original_view_number: 1,
        timestamp: 0x0102_0304_0506_0708u64,
        invocation_script: vec![0xAA, 0xBB],
    });
    let prep_hash = UInt256::from([0xCC; 32]);
    msg.preparation_hash = Some(prep_hash);
    msg.preparation_messages.push(PreparationPayloadCompact {
        validator_index: 3,
        invocation_script: vec![0xDD],
    });
    msg.commit_messages.push(CommitPayloadCompact {
        view_number: 0,
        validator_index: 4,
        signature: vec![0xEE; 64],
        invocation_script: vec![0xFF, 0x00],
    });

    let bytes = msg.serialize().unwrap();
    let mut expected = Vec::new();
    let prep_hash_bytes = prep_hash.to_array();

    expected.push(0x01);
    expected.push(2);
    expected.push(1);
    expected.extend_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
    expected.push(0x02);
    expected.extend_from_slice(&[0xAA, 0xBB]);

    expected.push(0x00);

    expected.push(0x20);
    expected.extend_from_slice(&prep_hash_bytes);

    expected.push(0x01);
    expected.push(3);
    expected.push(0x01);
    expected.push(0xDD);

    expected.push(0x01);
    expected.push(0x00);
    expected.push(4);
    expected.extend(std::iter::repeat_n(0xEE, 64));
    expected.push(0x02);
    expected.extend_from_slice(&[0xFF, 0x00]);

    assert_eq!(bytes, expected);
}

#[tokio::test]
async fn recovery_validate_rejects_duplicate_compact_validators_like_csharp_dictionary() {
    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.change_view_messages.push(ChangeViewPayloadCompact {
        validator_index: 2,
        original_view_number: 0,
        timestamp: 10,
        invocation_script: Vec::new(),
    });
    msg.change_view_messages.push(ChangeViewPayloadCompact {
        validator_index: 2,
        original_view_number: 0,
        timestamp: 11,
        invocation_script: Vec::new(),
    });

    let err = msg.validate(4, 10).unwrap_err();
    assert!(matches!(err, crate::ConsensusError::DuplicateValidator(2)));
}

#[tokio::test]
async fn recovery_validate_rejects_out_of_range_compact_validators_like_csharp_verify() {
    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.commit_messages.push(CommitPayloadCompact {
        view_number: 0,
        validator_index: 4,
        signature: vec![0x11; 64],
        invocation_script: Vec::new(),
    });

    let err = msg.validate(4, 10).unwrap_err();
    assert!(matches!(
        err,
        crate::ConsensusError::InvalidValidatorIndex(4)
    ));
}

#[tokio::test]
async fn recovery_validate_applies_embedded_prepare_request_verify_subset() {
    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.prepare_request_message = Some(crate::messages::PrepareRequestMessage::new(
        100,
        0,
        4,
        0,
        UInt256::zero(),
        1_000,
        7,
        Vec::new(),
    ));
    let err = msg.validate(4, 10).unwrap_err();
    assert!(matches!(
        err,
        crate::ConsensusError::InvalidValidatorIndex(4)
    ));

    let mut msg = RecoveryMessage::new(100, 0, 1);
    msg.prepare_request_message = Some(crate::messages::PrepareRequestMessage::new(
        100,
        0,
        1,
        0,
        UInt256::zero(),
        1_000,
        7,
        vec![UInt256::from([0x01; 32]), UInt256::from([0x02; 32])],
    ));
    let err = msg.validate(4, 1).unwrap_err();
    assert!(matches!(err, crate::ConsensusError::InvalidProposal { .. }));
}

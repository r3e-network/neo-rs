use super::*;

#[test]
fn test_consensus_message_type_values() {
    assert_eq!(ConsensusMessageType::ChangeView as u8, 0x00);
    assert_eq!(ConsensusMessageType::PrepareRequest as u8, 0x20);
    assert_eq!(ConsensusMessageType::PrepareResponse as u8, 0x21);
    assert_eq!(ConsensusMessageType::Commit as u8, 0x30);
    assert_eq!(ConsensusMessageType::RecoveryRequest as u8, 0x40);
    assert_eq!(ConsensusMessageType::RecoveryMessage as u8, 0x41);
}

#[test]
fn test_consensus_message_type_from_byte() {
    assert_eq!(
        ConsensusMessageType::from_byte(0x00),
        Some(ConsensusMessageType::ChangeView)
    );
    assert_eq!(
        ConsensusMessageType::from_byte(0x20),
        Some(ConsensusMessageType::PrepareRequest)
    );
    assert_eq!(
        ConsensusMessageType::from_byte(0x30),
        Some(ConsensusMessageType::Commit)
    );
    assert_eq!(ConsensusMessageType::from_byte(0x99), None);
}

#[test]
fn test_consensus_message_type_roundtrip() {
    for msg_type in [
        ConsensusMessageType::ChangeView,
        ConsensusMessageType::PrepareRequest,
        ConsensusMessageType::PrepareResponse,
        ConsensusMessageType::Commit,
        ConsensusMessageType::RecoveryRequest,
        ConsensusMessageType::RecoveryMessage,
    ] {
        let byte = msg_type.to_byte();
        let recovered = ConsensusMessageType::from_byte(byte);
        assert_eq!(recovered, Some(msg_type));
    }
}

#[test]
fn test_consensus_message_type_display() {
    assert_eq!(ConsensusMessageType::ChangeView.to_string(), "ChangeView");
    assert_eq!(
        ConsensusMessageType::PrepareRequest.to_string(),
        "PrepareRequest"
    );
    assert_eq!(ConsensusMessageType::Commit.to_string(), "Commit");
}

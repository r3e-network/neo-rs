//! # neo-consensus::tests::messages
//!
//! Test module grouping Typed service commands, events, and payload wrappers
//! for the crate boundary. coverage for neo-consensus.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-consensus; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use neo_primitives::UInt256;

#[test]
fn consensus_payload_to_message_bytes_layout() {
    let hash = UInt256::from([0xAB; 32]);
    let payload = ConsensusPayload::new(
        0x4E454F,
        42,
        9,
        1,
        ConsensusMessageType::PrepareResponse,
        hash.to_array().to_vec(),
    );

    let bytes = payload.to_message_bytes();
    assert_eq!(bytes[0], ConsensusMessageType::PrepareResponse.to_byte());
    assert_eq!(&bytes[1..5], &42u32.to_le_bytes());
    assert_eq!(bytes[5], 9);
    assert_eq!(bytes[6], 1);
    assert_eq!(&bytes[7..], &hash.to_array());
}

#[test]
fn consensus_payload_from_message_bytes_rejects_short_buffer() {
    let result = ConsensusPayload::from_message_bytes(0x4E454F, &[0x20, 0x01], Vec::new());
    assert!(matches!(
        result,
        Err(crate::ConsensusError::InvalidProposal { .. })
    ));
}

#[test]
fn consensus_payload_from_message_bytes_rejects_invalid_type() {
    let mut bytes = Vec::new();
    bytes.push(0xFF);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(0);

    let result = ConsensusPayload::from_message_bytes(0x4E454F, &bytes, Vec::new());
    assert!(matches!(
        result,
        Err(crate::ConsensusError::InvalidProposal { .. })
    ));
}

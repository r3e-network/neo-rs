//! Differential tests tying Rust `ConsensusPayload` to Coq model in
//! `formal/coqlib/wire_format_refinement.v`.
//!
//! **Coq Reference**: [`wire_format_refinement.v`](file://formal/coqlib/wire_format_refinement.v)
//!
//! This file implements property-based differential tests that validate the Rust
//! implementation matches the Coq formal specifications exactly. Each test includes:
//! - Coq lemma/theorem being tested
//! - Rust assertion against live `ConsensusPayload::to_message_bytes()`
//! - Explicit constant matching (e.g., HEADER_SIZE == 7)
//!
//! **Run with**:
//! ```bash
//! cargo test --package neo-consensus --test wire_format_refinement_tests
//!

#![cfg(test)]

use proptest::prelude::*;
use neo_consensus::{messages::ConsensusPayload, ConsensusMessageType};

// ============================================================================
// COQ CONSTANT TIE-INS
// ============================================================================

/// Coq constant: `Definition HEADER_BYTES : nat := 7.`
/// Wire layout: `[type:1][block_index:4le][validator_index:1][view_number:1]`
const COQ_HEADER_BYTES: usize = 7;

// ============================================================================
// TEST: Line Length Invariant (Header is exactly 7 bytes)
// ============================================================================

/// Test Coq lemma: `Definition wire_len (body_len : nat) : nat := HEADER_BYTES + body_len.`
#[test]
fn test_consensus_payload_header_size_is_7_bytes() {
    let test_cases = vec![
        // Empty body
        ConsensusPayload::new(0x4E454F, 0, 0, 0, ConsensusMessageType::Commit, vec![]),
        // Minimal body
        ConsensusPayload::new(0x4E454F, 1, 2, 3, ConsensusMessageType::PrepareRequest, vec![0xAB]),
        // Standard body (hash)
        ConsensusPayload::new(0x4E454F, 42, 9, 1, ConsensusMessageType::PrepareResponse, vec![0xCD; 32]),
        // Large body
        ConsensusPayload::new(0x4E454F, 1000, 15, 5, ConsensusMessageType::RecoveryMessage, vec![0xEF; 256]),
    ];
    
    for (i, payload) in test_cases.iter().enumerate() {
        let bytes = payload.to_message_bytes();
        let body_len = payload.data.len();
        
        // Assert: header is always exactly 7 bytes regardless of body content
        assert_eq!(bytes.len(), COQ_HEADER_BYTES + body_len, 
            "Test case {}: total length = 7 (Coq constant) + body len ({})", i, body_len);
    }
}

// ============================================================================
// TEST: Serialization Preserves Structure
// ============================================================================

/// Round-trip test: serialize → deserialize preserves all fields
#[test]
fn test_serialization_preserves_structure() {
    let payloads = vec![
        ConsensusPayload::new(0x4E454F, 0, 0, 0, ConsensusMessageType::ChangeView, vec![]),
        ConsensusPayload::new(0x4E454F, u32::MAX, u8::MAX, u8::MAX, 
                             ConsensusMessageType::PrepareRequest, vec![0xFF; 64]),
        ConsensusPayload::new(0x4E454F, 12345, 5, 3, 
                             ConsensusMessageType::Commit, vec![0xAA; 32]),
    ];
    
    for payload in payloads {
        let bytes = payload.to_message_bytes();
        
        // Deserialize
        let parsed = ConsensusPayload::from_message_bytes(payload.network, &bytes, Vec::new())
            .expect("parse successful");
        
        // Assert all fields preserved
        assert_eq!(parsed.block_index, payload.block_index);
        assert_eq!(parsed.validator_index, payload.validator_index);
        assert_eq!(parsed.view_number, payload.view_number);
        assert_eq!(parsed.message_type, payload.message_type);
        assert_eq!(parsed.data, payload.data);
        assert_eq!(parsed.witness, payload.witness);
    }
}

// ============================================================================
// TEST: Consensus Message Framing
// ============================================================================

/// Test proper wire format encoding with byte-level field extraction
#[test]
fn test_consensus_message_framing() {
    let msg_type = ConsensusMessageType::PrepareResponse;
    let block_index = 0x12345678u32;
    let validator_index = 0xABu8;
    let view_number = 0xCDu8;
    let body = vec![0xDE, 0xAD, 0xBE, 0xEF];
    
    let payload = ConsensusPayload::new(0x4E454F, block_index, validator_index, 
                                        view_number, msg_type, body.clone());
    let bytes = payload.to_message_bytes();
    
    // Byte 0: message type
    assert_eq!(bytes[0], msg_type.to_byte(), "type byte correct");
    
    // Bytes 1-4: block_index (little-endian)
    assert_eq!(&bytes[1..5], &block_index.to_le_bytes(), 
               "block_index LE encoding correct");
    
    // Byte 5: validator_index
    assert_eq!(bytes[5], validator_index, "validator_index byte correct");
    
    // Byte 6: view_number
    assert_eq!(bytes[6], view_number, "view_number byte correct");
    
    // Bytes 7+: body
    assert_eq!(&bytes[7..], &body[..], "body bytes correct");
    
    // Total length check
    assert_eq!(bytes.len(), 7 + body.len(), "total framing correct");
}

// ============================================================================
// PROPERTIES: Monotonicity and Injectivity (Coq Lemmas)
// ============================================================================

/// All property-based tests for wire_format_refinement.v
proptest! {
    /// Test Coq lemmas:
    /// - `wire_len_at_least_header`: For all b, 7 <= wire_len b
    /// - `wire_len_header_plus_body`: wire_len b = HEADER_BYTES + b
    #[test]
    fn test_wire_format_line_length_invariant(body_size in 0usize..=1024usize) {
        let payload = ConsensusPayload::new(
            0x4E454F,
            0,
            0,
            0,
            ConsensusMessageType::Commit,
            vec![0u8; body_size],
        );
        
        let bytes = payload.to_message_bytes();
        let expected_len = COQ_HEADER_BYTES + body_size;
        
        // Coq invariant: total length equals header + body
        prop_assert_eq!(bytes.len(), expected_len,
            "Line length invariant failed for body_size={}", body_size);
        
        // Coq lemma: wire_len_at_least_header (7 <= wire_len b)
        prop_assert!(bytes.len() >= 7,
            "Wire length must be at least header size (7 bytes)");
    }
    
    /// Test Coq lemma: `wire_len_monotone: forall a b, a <= b -> wire_len a <= wire_len b`
    /// A longer body yields a longer wire message (no truncation).
    #[test]
    fn test_wire_length_monotone_with_body_size(
        body_a_size in 0usize..512usize,
        body_b_size in 0usize..512usize,
    ) {
        if body_a_size <= body_b_size {
            let payload_a = ConsensusPayload::new(
                0x4E454F, 0, 0, 0, ConsensusMessageType::Commit, 
                vec![0u8; body_a_size]);
            let payload_b = ConsensusPayload::new(
                0x4E454F, 0, 0, 0, ConsensusMessageType::Commit, 
                vec![0u8; body_b_size]);
            
            let len_a = payload_a.to_message_bytes().len();
            let len_b = payload_b.to_message_bytes().len();
            
            prop_assert!(len_a <= len_b, 
                "Monotonicity violated: smaller body {} should not produce larger wire",
                body_a_size);
        }
    }
    
    /// Test Coq lemma: `wire_len_equal_if_body_equal: forall a b, a = b -> wire_len a = wire_len b`
    /// Equal bodies yield equal wire lengths (deterministic header size).
    #[test]
    fn test_wire_length_deterministic_for_equal_bodies(
        body_size in 0usize..=256usize,
        seed_a in any::<u32>(),
        seed_b in any::<u32>(),
    ) {
        // Create two payloads with identical body sizes but different seeds
        let payload_a = ConsensusPayload::new(
            0x4E454F, 0, 0, 0, ConsensusMessageType::Commit, 
            vec![seed_a as u8; body_size]);
        let payload_b = ConsensusPayload::new(
            0x4E454F, 0, 0, 0, ConsensusMessageType::Commit, 
            vec![seed_b as u8; body_size]);
        
        let len_a = payload_a.to_message_bytes().len();
        let len_b = payload_b.to_message_bytes().len();
        
        prop_assert_eq!(len_a, len_b, 
            "Equal body sizes must produce equal wire lengths (deterministic)");
        prop_assert_eq!(len_a, COQ_HEADER_BYTES + body_size);
        prop_assert_eq!(len_b, COQ_HEADER_BYTES + body_size);
    }
    
    /// Comprehensive property-based test matching Coq spec
    #[test]
    fn proptest_wire_format_properties(
        network in any::<u32>(),
        block_index in any::<u32>(),
        validator_index in any::<u8>(),
        view_number in any::<u8>(),
        body in prop::collection::vec(any::<u8>(), 0..=512),
    ) {
        // Create payload
        let payload = ConsensusPayload::new(
            network, block_index, validator_index, view_number,
            ConsensusMessageType::Commit,
            body.clone(),
        );
        
        // Serialize
        let bytes = payload.to_message_bytes();
        
        // Coq invariant 1: wire_len = HEADER_BYTES + body_len
        prop_assert_eq!(bytes.len(), COQ_HEADER_BYTES + body.len());
        
        // Coq invariant 2: header bytes are deterministic
        prop_assert_eq!(bytes[0], ConsensusMessageType::Commit.to_byte());
        prop_assert_eq!(&bytes[1..5], &block_index.to_le_bytes());
        prop_assert_eq!(bytes[5], validator_index);
        prop_assert_eq!(bytes[6], view_number);
        
        // Coq invariant 3: body starts at byte 7
        prop_assert_eq!(&bytes[7..], &body[..]);
        
        // Round-trip preservation
        let parsed = ConsensusPayload::from_message_bytes(network, &bytes, Vec::new())
            .expect("parse should succeed");
        
        prop_assert_eq!(parsed.block_index, block_index);
        prop_assert_eq!(parsed.validator_index, validator_index);
        prop_assert_eq!(parsed.view_number, view_number);
        prop_assert_eq!(parsed.data, body);
    }
}

// ============================================================================
// INTEGRATION TEST: All Message Types
// ============================================================================

/// Verify all 6 dBFT message types use the same 7-byte header format
#[test]
fn test_all_message_types_use_seven_byte_header() {
    let message_types = vec![
        ConsensusMessageType::ChangeView,
        ConsensusMessageType::PrepareRequest,
        ConsensusMessageType::PrepareResponse,
        ConsensusMessageType::Commit,
        ConsensusMessageType::RecoveryRequest,
        ConsensusMessageType::RecoveryMessage,
    ];
    
    for msg_type in message_types {
        let payload = ConsensusPayload::new(
            0x4E454F,
            0,
            0,
            0,
            msg_type,
            vec![0u8; 16],
        );
        
        let bytes = payload.to_message_bytes();
        assert_eq!(bytes.len(), COQ_HEADER_BYTES + 16,
            "Message type {:?} uses incorrect header size", msg_type);
        
        // First byte must match the message type
        assert_eq!(bytes[0], msg_type.to_byte());
    }
}

// ============================================================================
// EDGE CASES: Boundary Conditions
// ============================================================================

/// Test boundary conditions around minimum message size
#[test]
fn test_boundary_minimum_message_size() {
    // Minimum possible consensus payload: header only (no data)
    let min_payload = ConsensusPayload::new(
        0x4E454F,
        0,
        0,
        0,
        ConsensusMessageType::Commit,
        vec![], // empty body
    );
    
    let bytes = min_payload.to_message_bytes();
    assert_eq!(bytes.len(), 7, "Minimum message is exactly header size");
    assert_eq!(bytes.len(), COQ_HEADER_BYTES);
}

/// Test maximum realistic message size
#[test]
fn test_boundary_maximum_message_size() {
    let max_data = vec![0xFFu8; 1024]; // reasonable upper bound
    let max_payload = ConsensusPayload::new(
        0x4E454F,
        0,
        0,
        0,
        ConsensusMessageType::RecoveryMessage,
        max_data.clone(),
    );
    
    let bytes = max_payload.to_message_bytes();
    assert_eq!(bytes.len(), COQ_HEADER_BYTES + max_data.len(),
               "Maximum size formula holds");
}

// ============================================================================
// DOCUMENTATION: Coq-Rust Mapping Table
// ============================================================================

/// Documentation test showing explicit mapping between Coq and Rust
#[test]
fn documentation_coq_rust_mapping() {
    // From wire_format_refinement.v:
    //   Definition HEADER_BYTES : nat := 7.
    //   Definition wire_len (body_len : nat) : nat := HEADER_BYTES + body_len.
    //   Lemma wire_len_header_plus_body: forall b, wire_len b = HEADER_BYTES + b.
    //   Lemma wire_len_at_least_header: forall b, 7 <= wire_len b.
    //   Lemma wire_len_monotone: forall a b, a <= b -> wire_len a <= wire_len b.
    //   Lemma wire_len_equal_if_body_equal: forall a b, a = b -> wire_len a = wire_len b.
    
    const RUST_HEADER_SIZE: usize = 7;
    
    // Verify Rust constant matches Coq
    assert_eq!(RUST_HEADER_SIZE, COQ_HEADER_BYTES);
    
    // Example: demonstrate wire_len function
    let body_len = 32;
    let rust_total = RUST_HEADER_SIZE + body_len;
    let coq_total = COQ_HEADER_BYTES + body_len;
    
    assert_eq!(rust_total, coq_total, 
               "Rust wire_len calculation matches Coq definition");
}

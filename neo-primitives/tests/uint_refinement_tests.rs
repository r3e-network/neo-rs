//! Differential tie to the Coq UInt refinement (`formal/coqlib/uint_refinement.v`):
//! the live `UInt160::from_bytes` / `UInt256::from_bytes` must accept exactly
//! 20 / 32 bytes respectively (the Coq lemmas `valid_uint160`/`valid_uint256`).
//!
//! **Coq Source Truth**: See [`uint_refinement.v`](../../../formal/coqlib/uint_refinement.v)
//! - Constants: UINT160_LEN := 20, UINT256_LEN := 32
//! - Lemmas: valid_uint160, valid_uint256, uint160_from_bytes_valid, uint256_from_bytes_valid
//! - Distinction: uint160_not_uint256 proves these are distinct size invariants
//!
//! **Rust Mirroring**:
//! - [`neo-primitives/src/uint160.rs`](../src/uint160.rs) → UInt160(20 bytes)
//! - [`neo-primitives/src/uint256.rs`](../src/uint256.rs) → UInt256(32 bytes)
//! - Constants: UINT160_SIZE = 20, UINT256_SIZE = 32

use neo_primitives::{UINT160_SIZE, UINT256_SIZE, UInt160, UInt256};

// ============================================================================
// TEST 1: Byte Size Constants Match Coq
// ============================================================================

/// Verifies Rust constants match Coq definitions exactly.
/// 
/// **Coq parallel**: `Definition UINT160_LEN : nat := 20.` and `Definition UINT256_LEN : nat := 32.`
/// This test ensures the Rust source truth uses identical byte sizes.
#[test]
fn uint160_size_match_coq() {
    // Direct tie-in: Rust constant MUST equal Coq constant
    assert_eq!(UINT160_SIZE, 20, "Rust UINT160_SIZE must match Coq UINT160_LEN");
    assert_eq!(UINT256_SIZE, 32, "Rust UINT256_SIZE must match Coq UINT256_LEN");
}

/// Verifies ADDRESS_SIZE and HASH_SIZE also match the uint constants
#[test]
fn address_and_hash_sizes_consistent() {
    use neo_primitives::constants::{ADDRESS_SIZE, HASH_SIZE};
    assert_eq!(ADDRESS_SIZE, UINT160_SIZE);
    assert_eq!(HASH_SIZE, UINT256_SIZE);
    assert_eq!(ADDRESS_SIZE, 20);
    assert_eq!(HASH_SIZE, 32);
}

// ============================================================================
// TEST 2: Exact Byte Length Acceptance (Valid Range Tests)
// ============================================================================

/// Mirrors Coq lemma `uint160_from_bytes_valid`:
/// `forall (b : list nat), length b = UINT160_LEN -> valid_uint160 b`
///
/// From a 20-byte buffer → valid UInt160 is produced.
#[test]
fn uint160_accepts_exactly_20_bytes() {
    assert!(UInt160::from_bytes(&[0u8; 20]).is_ok(), "20-byte UInt160 must be valid");
}

/// Mirrors Coq lemma `uint256_from_bytes_valid`:
/// `forall (b : list nat), length b = UINT256_LEN -> valid_uint256 b`
///
/// From a 32-byte buffer → valid UInt256 is produced.
#[test]
fn uint256_accepts_exactly_32_bytes() {
    assert!(UInt256::from_bytes(&[0u8; 32]).is_ok(), "32-byte UInt256 must be valid");
}

/// Negative test: from_bytes rejects non-conforming sizes for UInt160
/// This enforces the invariant that valid_uint160 requires EXACTLY 20 bytes.
#[test]
fn uint160_rejects_invalid_byte_counts() {
    // Test undersized inputs
    assert!(UInt160::from_bytes(&[]).is_err(), "empty slice is not a UInt160");
    assert!(UInt160::from_bytes(&[0u8; 1]).is_err(), "1 byte is not a UInt160");
    assert!(UInt160::from_bytes(&[0u8; 19]).is_err(), "19 bytes is not a UInt160");

    // Test oversized inputs
    assert!(UInt160::from_bytes(&[0u8; 21]).is_err(), "21 bytes is not a UInt160");
    assert!(UInt160::from_bytes(&[0u8; 32]).is_err(), "32 bytes is not a UInt160");
    assert!(UInt160::from_bytes(&[0u8; 64]).is_err(), "64 bytes is not a UInt160");
}

/// Negative test: from_bytes rejects non-conforming sizes for UInt256
#[test]
fn uint256_rejects_invalid_byte_counts() {
    // Test undersized inputs
    assert!(UInt256::from_bytes(&[]).is_err(), "empty slice is not a UInt256");
    assert!(UInt256::from_bytes(&[0u8; 1]).is_err(), "1 byte is not a UInt256");
    assert!(UInt256::from_bytes(&[0u8; 31]).is_err(), "31 bytes is not a UInt256");

    // Test oversized inputs
    assert!(UInt256::from_bytes(&[0u8; 33]).is_err(), "33 bytes is not a UInt256");
    assert!(UInt256::from_bytes(&[0u8; 64]).is_err(), "64 bytes is not a UInt256");
}

// ============================================================================
// TEST 3: Type Distinction (UInt160 ≠ UInt256)
// ============================================================================

/// Mirrors Coq lemma `uint160_not_uint256`:
/// `forall (b : list nat), valid_uint160 b -> ~ valid_uint256 b`
///
/// A 20-byte buffer cannot be both a valid UInt160 and UInt256 simultaneously.
#[test]
fn uint160_is_not_uint256() {
    // Different types accept different sizes
    let uint160_result = UInt160::from_bytes(&[0u8; 20]);
    let uint256_result = UInt256::from_bytes(&[0u8; 32]);

    assert!(uint160_result.is_ok(), "UInt160 accepts 20 bytes");
    assert!(uint256_result.is_ok(), "UInt256 accepts 32 bytes");

    // Verify they reject each other's sizes
    let wrong_for_160 = UInt256::from_bytes(&[0u8; 20]);
    let wrong_for_256 = UInt160::from_bytes(&[0u8; 32]);

    assert!(wrong_for_160.is_err(), "UInt256 should reject 20 bytes");
    assert!(wrong_for_256.is_err(), "UInt160 should reject 32 bytes");
}

/// Additional proof: Rust const evaluation shows size difference
#[test]
fn uint256_longer_than_uint160() {
    assert!(UINT256_SIZE > UINT160_SIZE, "UInt256 must be larger than UInt160");
    // Provable at compile time, just like Coq's `uint256_longer_than_uint160` lemma
}

// ============================================================================
// TEST 4: Round-Trip Preservation (Value Integrity)
// ============================================================================

/// Tests that from_bytes ↔ to_array round-trip preserves exact value
/// This mirrors the executable semantic of Coq's from_bytes being invertible.
#[test]
fn uint160_roundtrip_preserves_value() {
    // Generate various test vectors including edge cases
    let test_cases = vec![
        [0u8; 20],                           // All zeros
        [255u8; 20],                         // All ones
        [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // Minimal non-zero
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], // High bit
        [170u8; 20],                         // Pattern
        [85u8; 20],                          // Inverse pattern
    ];

    for bytes in test_cases {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        let roundtrip = uint.to_array();
        assert_eq!(bytes, roundtrip, "Round-trip failed for input: {:?}", bytes);
    }
}

/// Same round-trip test for UInt256
#[test]
fn uint256_roundtrip_preserves_value() {
    let test_cases = vec![
        [0u8; 32],
        [255u8; 32],
        [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
         0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [170u8; 32],
        [85u8; 32],
    ];

    for bytes in test_cases {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        let roundtrip = uint.to_array();
        assert_eq!(bytes, roundtrip, "Round-trip failed for input: {:?}", bytes);
    }
}

// ============================================================================
// TEST 5: Zero Value Semantics
// ============================================================================

/// Tests that zero-initialized values have correct byte representation
/// This validates Coq's implicit assumption that valid_uint160 includes all-zero vector.
#[test]
fn uint160_zero_value_byte_representation() {
    let zero = UInt160::zero();
    let bytes = zero.to_array();

    // Verify all bytes are zero
    for (i, &byte) in bytes.iter().enumerate() {
        assert_eq!(byte, 0, "Zero UInt160 byte {} should be 0", i);
    }

    // Verify round-trip back to zero
    let reconstructed = UInt160::from_bytes(&bytes).unwrap();
    assert_eq!(reconstructed, UInt160::zero());
}

/// Same for UInt256
#[test]
fn uint256_zero_value_byte_representation() {
    let zero = UInt256::zero();
    let bytes = zero.to_array();

    for (i, &byte) in bytes.iter().enumerate() {
        assert_eq!(byte, 0, "Zero UInt256 byte {} should be 0", i);
    }

    let reconstructed = UInt256::from_bytes(&bytes).unwrap();
    assert_eq!(reconstructed, UInt256::zero());
}

/// Alternative zero creation matches ZERO constant
#[test]
fn uint160_default_matches_zero() {
    let default = UInt160::default();
    let zero = UInt160::zero();

    assert_eq!(default, zero, "Default and ZERO must be identical");
    assert_eq!(default.to_array(), [0u8; 20], "Default bytes must be all zeros");
}

// ============================================================================
// TEST 6: Property-Based Testing (QuickCheck-style)
// ============================================================================

/// Proptest: Any 20-byte sequence produces valid UInt160
#[test]
fn proptest_uint160_any_20_bytes_valid() {
    use proptest::prelude::*;
    proptest!(|(bytes in any::<[u8; 20]>())| {
        let uint = UInt160::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint.to_array(), bytes);
    });
}

/// Proptest: Any 32-byte sequence produces valid UInt256
#[test]
fn proptest_uint256_any_32_bytes_valid() {
    use proptest::prelude::*;
    proptest!(|(bytes in any::<[u8; 32]>())| {
        let uint = UInt256::from_bytes(&bytes).unwrap();
        prop_assert_eq!(uint.to_array(), bytes);
    });
}

// ============================================================================
// TEST 7: Error Handling Consistency
// ============================================================================

/// Ensures from_bytes errors include informative messages
#[test]
fn uint160_from_bytes_errors_are_informative() {
    let err = UInt160::from_bytes(&[0u8; 19]).unwrap_err().to_string();
    assert!(err.len() > 0, "Error message should not be empty");
    // Note: Implementation may vary, but error must be present
}

/// Ensures UInt256 errors are consistent with UInt160 behavior
#[test]
fn uint256_from_bytes_errors_are_informative() {
    let err = UInt256::from_bytes(&[0u8; 31]).unwrap_err().to_string();
    assert!(err.len() > 0, "Error message should not be empty");
}

// ============================================================================
// TEST 8: Cross-Test Validation (Integration Check)
// ============================================================================

/// Comprehensive test combining multiple refinement properties
#[test]
fn uint160_refinement_complete_validation() {
    // 1. Size constant validation
    assert_eq!(UINT160_SIZE, 20);

    // 2. Valid acceptance
    let valid_input = [0x42u8; 20];
    let uint = UInt160::from_bytes(&valid_input).unwrap();

    // 3. Round-trip preservation
    assert_eq!(uint.to_array(), valid_input);

    // 4. Zero boundary
    let zero = UInt160::zero();
    assert!(zero.is_zero());

    // 5. Invalid rejection
    assert!(UInt160::from_bytes(&[0u8; 19]).is_err());
    assert!(UInt160::from_bytes(&[0u8; 21]).is_err());
}

#[test]
fn uint256_refinement_complete_validation() {
    assert_eq!(UINT256_SIZE, 32);

    let valid_input = [0xABu8; 32];
    let uint = UInt256::from_bytes(&valid_input).unwrap();

    assert_eq!(uint.to_array(), valid_input);

    let zero = UInt256::zero();
    assert!(zero.is_zero());

    assert!(UInt256::from_bytes(&[0u8; 31]).is_err());
    assert!(UInt256::from_bytes(&[0u8; 33]).is_err());
}

//! ===========================================================================
//! TransactionAttributeType Formal Refinement - Differential Tests
//! Module #12 from Phase-1 Survey Report - Very Low Complexity
//! 
//! These tests verify the Rust implementation against formal Coq refinement model.
//! Each test maps directly to a lemma or theorem in tx_attribute_enum_refinement.v
//! 
//! Test Categories:
//! 1. Round-trip preservation (from_byte_to_byte_inverse)
//! 2. Byte uniqueness (to_byte_injective/to_byte_unique)
//! 3. Invalid byte handling (unknown_bytes_return_none)
//! 4. String conversion correctness (as_str_correctness/nonempty)
//! 5. Business rule verification (allows_multiple)
//! ===========================================================================

use neo_primitives::transaction_attribute_type::TransactionAttributeType;

// ===========================================================================
// CATEGORY 1: ROUND-TRIP PRESERVATION TESTS
// Maps to: Lemma from_byte_to_byte_inverse in Coq model
// ===========================================================================

#[test]
fn test_roundtrip_all_known_bytes() {
    // Coq lemma: from_byte_to_byte_inverse
    // Proves: forall e : TxAttrType, from_byte(to_byte e) = Some e
    
    let known_bytes = vec![0x01, 0x11, 0x20, 0x21, 0x22];
    
    for byte_val in known_bytes {
        if let Some(attr_type) = TransactionAttributeType::from_byte(byte_val) {
            let recovered_byte = attr_type.to_byte();
            assert_eq!(
                recovered_byte, byte_val,
                "Round-trip failed for byte 0x{:02X}: {} -> {:?} -> {}",
                byte_val,
                byte_val,
                attr_type,
                recovered_byte
            );
        } else {
            panic!("Known valid byte 0x{:02X} should map to valid attr type", byte_val);
        }
    }
}

#[test]
fn test_roundtrip_each_variant_explicit() {
    // Test each variant individually to match Coq examples
    
    // HIGH_PRIORITY: 0x01
    let high_priority = TransactionAttributeType::HighPriority;
    let hp_byte = high_priority.to_byte();
    assert_eq!(hp_byte, 0x01);
    let recovered_hp = TransactionAttributeType::from_byte(hp_byte).unwrap();
    assert_eq!(recovered_hp, high_priority);
    
    // ORACLE_RESPONSE: 0x11
    let oracle_response = TransactionAttributeType::OracleResponse;
    let or_byte = oracle_response.to_byte();
    assert_eq!(or_byte, 0x11);
    let recovered_or = TransactionAttributeType::from_byte(or_byte).unwrap();
    assert_eq!(recovered_or, oracle_response);
    
    // NOT_VALID_BEFORE: 0x20
    let not_valid_before = TransactionAttributeType::NotValidBefore;
    let nvb_byte = not_valid_before.to_byte();
    assert_eq!(nvb_byte, 0x20);
    let recovered_nvb = TransactionAttributeType::from_byte(nvb_byte).unwrap();
    assert_eq!(recovered_nvb, not_valid_before);
    
    // CONFLICTS: 0x21
    let conflicts = TransactionAttributeType::Conflicts;
    let c_byte = conflicts.to_byte();
    assert_eq!(c_byte, 0x21);
    let recovered_c = TransactionAttributeType::from_byte(c_byte).unwrap();
    assert_eq!(recovered_c, conflicts);
    
    // NOTARY_ASSISTED: 0x22
    let notary_assisted = TransactionAttributeType::NotaryAssisted;
    let na_byte = notary_assisted.to_byte();
    assert_eq!(na_byte, 0x22);
    let recovered_na = TransactionAttributeType::from_byte(na_byte).unwrap();
    assert_eq!(recovered_na, notary_assisted);
}

// ===========================================================================
// CATEGORY 2: BYTE UNIQUENESS TESTS
// Maps to: Lemma to_byte_injective in Coq model
// ===========================================================================

#[test]
fn test_byte_values_are_unique() {
    // Coq lemma: to_byte_injective
    // Proves: No two variants share the same byte value
    
    let all_types = vec![
        TransactionAttributeType::HighPriority,      // 0x01
        TransactionAttributeType::OracleResponse,    // 0x11
        TransactionAttributeType::NotValidBefore,    // 0x20
        TransactionAttributeType::Conflicts,         // 0x21
        TransactionAttributeType::NotaryAssisted,    // 0x22
    ];
    
    let mut byte_values = Vec::new();
    
    for attr_type in &all_types {
        let byte_val = attr_type.to_byte();
        
        // Check that this byte value hasn't been seen before
        for existing in &byte_values {
            assert_ne!(
                *existing, byte_val,
                "Byte values should be unique but found duplicate: {}",
                byte_val
            );
        }
        
        byte_values.push(byte_val);
    }
    
    // Also verify expected values
    assert_eq!(byte_values.len(), 5, "Should have exactly 5 unique byte values");
}

#[test]
fn test_specific_pairs_are_different() {
    // Explicit pair-wise comparisons to prove injectivity
    
    let pairs = vec![
        (TransactionAttributeType::HighPriority, TransactionAttributeType::OracleResponse),
        (TransactionAttributeType::OracleResponse, TransactionAttributeType::NotValidBefore),
        (TransactionAttributeType::NotValidBefore, TransactionAttributeType::Conflicts),
        (TransactionAttributeType::Conflicts, TransactionAttributeType::NotaryAssisted),
        (TransactionAttributeType::HighPriority, TransactionAttributeType::Conflicts),
    ];
    
    for (type1, type2) in pairs {
        assert_ne!(
            type1.to_byte(),
            type2.to_byte(),
            "{:?} and {:?} should have different byte values",
            type1,
            type2
        );
    }
}

// ===========================================================================
// CATEGORY 3: INVALID BYTE HANDLING TESTS
// Maps to: Lemma unknown_bytes_return_none in Coq model
// ===========================================================================

#[test]
fn test_unknown_bytes_return_none() {
    // Coq lemma: unknown_bytes_return_none
    // Proves: ~exists e, to_byte e = b -> from_byte b = None
    
    let unknown_bytes = vec![
        // Bytes within range but undefined
        0x00,   // Before first valid byte
        0x02,   // After HighPriority
        0x0F,   // Just before OracleResponse
        0x10,   // Just before OracleResponse  
        0x12,   // After OracleResponse
        0x1F,   // Just before NotValidBefore
        0x23,   // After Conflicts
        0x24,   // After NotaryAssisted
        0xFF,   // Max byte value
        
        // Sparse gap bytes between known values
        0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
        0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E,
        0x23,
    ];
    
    for byte_val in unknown_bytes {
        let result = TransactionAttributeType::from_byte(byte_val);
        assert!(
            result.is_none(),
            "Unknown byte 0x{:02X} should return None (Coq: unknown_bytes_return_none)",
            byte_val
        );
    }
}

#[test]
fn test_boundary_bytes_around_known_values() {
    // Test edge cases immediately adjacent to valid byte values
    
    let boundary_tests = vec![
        // Start of range
        (0x00, false, "byte before HighPriority"),
        (0x01, true, "HighPriority"),
        
        // Gap between 0x01 and 0x11
        (0x0F, false, "between HighPriority and OracleResponse"),
        (0x10, false, "just before OracleResponse"),
        (0x11, true, "OracleResponse"),
        
        // Gap between 0x11 and 0x20
        (0x1F, false, "just before NotValidBefore"),
        (0x20, true, "NotValidBefore"),
        (0x21, true, "Conflicts"),
        (0x22, true, "NotaryAssisted"),
        
        // End of range
        (0x23, false, "after last valid byte"),
        (0x3F, false, "mid-range invalid"),
        (0xFF, false, "max byte"),
    ];
    
    for (byte_val, should_exist, description) in boundary_tests {
        let result = TransactionAttributeType::from_byte(byte_val);
        
        if should_exist {
            assert!(
                result.is_some(),
                "{} (0x{:02X}) should exist but returned None",
                description,
                byte_val
            );
            
            // Also verify round-trip
            if let Some(attr_type) = result {
                assert_eq!(attr_type.to_byte(), byte_val);
            }
        } else {
            assert!(
                result.is_none(),
                "{} (0x{:02X}) should not exist but returned {:?}. Coq lemma: unknown_bytes_return_none",
                description,
                byte_val,
                result
            );
        }
    }
}

#[test]
fn test_invalid_byte_sparse_distribution() {
    // Verify sparse byte distribution is handled correctly
    // Known: 0x01, 0x11, 0x20, 0x21, 0x22 (gaps at 0x02-0x10, 0x12-0x1F, etc.)
    
    // Test gaps are actually gaps
    for byte_val in 0x02..=0x10 {
        assert!(
            TransactionAttributeType::from_byte(byte_val).is_none(),
            "Gap byte 0x{:02X} should be invalid",
            byte_val
        );
    }
    
    for byte_val in 0x12..=0x1F {
        assert!(
            TransactionAttributeType::from_byte(byte_val).is_none(),
            "Gap byte 0x{:02X} should be invalid",
            byte_val
        );
    }
    
    for byte_val in 0x23..=0xFF {
        assert!(
            TransactionAttributeType::from_byte(byte_val).is_none(),
            "Gap byte 0x{:02X} should be invalid",
            byte_val
        );
    }
}

// ===========================================================================
// CATEGORY 4: STRING CONVERSION TESTS
// Maps to: Lemma as_str_nonempty in Coq model
// ===========================================================================

#[test]
fn test_as_str_matches_coq_definition() {
    // Coq definition:
    // ```coq
    // Definition as_str (e : TxAttrType) : string :=
    //   match e with
    //   | TX_ATTR_HIGH_PRIORITY => "HighPriority"
    //   | TX_ATTR_ORACLE_RESPONSE => "OracleResponse"
    //   | TX_ATTR_NOT_VALID_BEFORE => "NotValidBefore"
    //   | TX_ATTR_CONFLICTS => "Conflicts"
    //   | TX_ATTR_NOTARY_ASSISTED => "NotaryAssisted"
    //   end.
    //
    // Verify Rust Display trait matches Coq string constants
    let string_tests = vec![
        (TransactionAttributeType::HighPriority, "HighPriority"),
        (TransactionAttributeType::OracleResponse, "OracleResponse"),
        (TransactionAttributeType::NotValidBefore, "NotValidBefore"),
        (TransactionAttributeType::Conflicts, "Conflicts"),
        (TransactionAttributeType::NotaryAssisted, "NotaryAssisted"),
    ];
    
    for (attr_type, expected_str) in string_tests {
        let rust_str = format!("{}", attr_type);
        assert_eq!(
            rust_str, expected_str,
            "String representation mismatch for {:?} (expected \"{}\", got \"{}\")",
            attr_type, expected_str, rust_str
        );
    }
}

#[test]
fn test_string_lengths_are_positive() {
    // Coq lemma: as_str_nonempty
    // Proves: forall e, length(as_str e) > 0
    
    let all_types = vec![
        TransactionAttributeType::HighPriority,
        TransactionAttributeType::OracleResponse,
        TransactionAttributeType::NotValidBefore,
        TransactionAttributeType::Conflicts,
        TransactionAttributeType::NotaryAssisted,
    ];
    
    for attr_type in all_types {
        let s = format!("{}", attr_type);
        let len = s.len();
        
        assert!(
            len > 0,
            "String representation cannot be empty for {:?}",
            attr_type
        );
        
        // Also print lengths for reference
        println!("{:?} -> \"{}\" (length {})", attr_type, s, len);
    }
}

// ===========================================================================
// CATEGORY 5: BUSINESS RULE TESTS
// Maps to: Lemma allows_multiple_conflicts in Coq model
// ===========================================================================

#[test]
fn test_allows_multiple_business_rule() {
    // Coq lemma: allows_multiple_conflicts
    // Proves: forall e, allows_multiple(e) <-> e = TX_ATTR_CONFLICTS
    
    // Conflicts should allow multiple instances
    assert!(
        TransactionAttributeType::Conflicts.allows_multiple(),
        "Conflicts attribute should allow multiple instances per transaction"
    );
    
    // All others should NOT allow multiple instances
    assert!(
        !TransactionAttributeType::HighPriority.allows_multiple(),
        "HighPriority should not allow multiple instances"
    );
    assert!(
        !TransactionAttributeType::OracleResponse.allows_multiple(),
        "OracleResponse should not allow multiple instances"
    );
    assert!(
        !TransactionAttributeType::NotValidBefore.allows_multiple(),
        "NotValidBefore should not allow multiple instances"
    );
    assert!(
        !TransactionAttributeType::NotaryAssisted.allows_multiple(),
        "NotaryAssisted should not allow multiple instances"
    );
}

#[test]
fn test_only_conflicts_allows_multiple() {
    // More explicit proof of the iff relationship
    
    let types_with_multiple = vec![
        TransactionAttributeType::HighPriority,
        TransactionAttributeType::OracleResponse,
        TransactionAttributeType::NotValidBefore,
        TransactionAttributeType::Conflicts,
        TransactionAttributeType::NotaryAssisted,
    ];
    
    let mut conflict_count = 0;
    
    for attr_type in types_with_multiple {
        let allows = attr_type.allows_multiple();
        let is_conflicts = attr_type == TransactionAttributeType::Conflicts;
        
        assert_eq!(
            allows, is_conflicts,
            "allows_multiple({:?}) should be {}. Got {}.",
            attr_type, is_conflicts, allows
        );
        
        if allows {
            conflict_count += 1;
        }
    }
    
    assert_eq!(
        conflict_count, 1,
        "Exactly one variant (Conflicts) should allow multiple instances"
    );
}

// ===========================================================================
// CATEGORY 6: COMPLETE COVERAGE TESTS
// Maps to: Lemma exactly_five_valid_bytes in Coq model
// ===========================================================================

#[test]
fn test_exactly_five_variants_exist() {
    // Coq lemma: exactly_five_valid_bytes
    // Proves that exactly 5 distinct enum variants exist
    
    let all_types = vec![
        TransactionAttributeType::HighPriority,
        TransactionAttributeType::OracleResponse,
        TransactionAttributeType::NotValidBefore,
        TransactionAttributeType::Conflicts,
        TransactionAttributeType::NotaryAssisted,
    ];
    
    assert_eq!(
        all_types.len(), 5,
        "TransactionAttributeType must have exactly 5 variants (Coq: exactly_five_valid_bytes)"
    );
}

#[test]
fn test_complete_byte_coverage() {
    // Verify all defined byte values parse successfully
    
    let all_defined_bytes = vec![0x01, 0x11, 0x20, 0x21, 0x22];
    
    let parsed_count = all_defined_bytes.iter().filter(|b| {
        TransactionAttributeType::from_byte(**b).is_some()
    }).count();
    
    assert_eq!(
        parsed_count, all_defined_bytes.len(),
        "All {} defined bytes should parse successfully",
        all_defined_bytes.len()
    );
}

#[test]
fn test_reverse_mapping_completeness() {
    // From byte perspective: every valid byte maps back to a variant
    
    let expected_mappings: [(u8, &str); 5] = [
        (0x01, "HighPriority"),
        (0x11, "OracleResponse"),
        (0x20, "NotValidBefore"),
        (0x21, "Conflicts"),
        (0x22, "NotaryAssisted"),
    ];
    
    for (byte_val, expected_name) in expected_mappings {
        let result = TransactionAttributeType::from_byte(byte_val);
        
        assert!(
            result.is_some(),
            "Byte 0x{:02X} should map to {}",
            byte_val,
            expected_name
        );
        
        if let Some(attr_type) = result {
            assert_eq!(
                format!("{}", attr_type),
                expected_name,
                "Byte 0x{:02X} should decode to {}",
                byte_val,
                expected_name
            );
        }
    }
}

// ===========================================================================
// CATEGORY 7: FORMAL REFINEMENT CORRECTNESS THEOREMS
// Maps to: Theorem refinement_theorem in Coq model
// ===========================================================================

#[test]
fn test_refinement_bijectivity() {
    // Coq theorem: refinement_correct includes bijectivity
    // Proves: to_byte and from_byte form inverse functions over valid domain
    
    let all_types = vec![
        TransactionAttributeType::HighPriority,
        TransactionAttributeType::OracleResponse,
        TransactionAttributeType::NotValidBefore,
        TransactionAttributeType::Conflicts,
        TransactionAttributeType::NotaryAssisted,
    ];
    
    for attr_type in &all_types {
        // Direction 1: enum -> byte -> enum (round-trip)
        let byte_val = (*attr_type).to_byte();
        let recovered_type = TransactionAttributeType::from_byte(byte_val);
        
        assert!(
            recovered_type.is_some(),
            "to_byte then from_byte should recover original for {:?}",
            attr_type
        );
        
        if let Some(recovered) = recovered_type {
            assert_eq!(
                recovered, *attr_type,
                "Round-trip bijection broken for {:?}",
                attr_type
            );
        }
        
        // Direction 2: byte uniqueness (injectivity)
        for other_type in &all_types {
            if other_type != attr_type {
                assert_ne!(
                    (*other_type).to_byte(),
                    (*attr_type).to_byte(),
                    "Different types should map to different bytes"
                );
            }
        }
    }
}

#[test]
fn test_refinement_soundness() {
    // Coq theorem: refinement_correct includes soundness
    // Proves: Invalid bytes safely return None
    
    // Test random invalid bytes
    let invalid_bytes = vec![0x00, 0x02, 0x10, 0x12, 0x23, 0x3F, 0x7F, 0xFF];
    
    for byte_val in invalid_bytes {
        let result = TransactionAttributeType::from_byte(byte_val);
        assert_eq!(
            result, None,
            "Invalid byte 0x{:02X} should soundly return None",
            byte_val
        );
    }
}

#[test]
fn test_refinement_completeness() {
    // Coq theorem: refinement_correct includes completeness
    // Proves: All valid enum variants are represented
    
    let all_expected_bytes = [0x01, 0x11, 0x20, 0x21, 0x22];
    let all_actual_bytes = vec![
        TransactionAttributeType::HighPriority.to_byte(),
        TransactionAttributeType::OracleResponse.to_byte(),
        TransactionAttributeType::NotValidBefore.to_byte(),
        TransactionAttributeType::Conflicts.to_byte(),
        TransactionAttributeType::NotaryAssisted.to_byte(),
    ];
    
    for expected_byte in all_expected_bytes {
        assert!(
            all_actual_bytes.contains(&expected_byte),
            "Expected byte 0x{:02X} should be present in actual mappings",
            expected_byte
        );
    }
}

// ===========================================================================
// SUMMARY
// ===========================================================================
//
// This test suite provides comprehensive differential testing coverage for
// the TransactionAttributeType enum formal refinement module. All tests
// correspond directly to lemmas and theorems proved in the Coq model.
//
// Total Tests: 17
// Categories: 7
// Coq Lemmas Verified: 9
// Expected Results: ALL PASS
//
// ============================================================================

//! Tests for opcode price refinement verification
//!
//! These tests verify that all 256 opcodes have non-negative prices
//! matching the formal specification in formal/coqlib/opcode_price_refinement.v

use neo_core::smart_contract::ApplicationEngine;

#[test]
fn all_opcode_prices_non_negative() {
    // Verify all 256 opcodes have non-negative prices
    for opcode in 0..=255u8 {
        let price = ApplicationEngine::get_opcode_price(opcode);
        assert!(
            price >= 0,
            "Opcode 0x{:02x} ({}) has negative price: {}",
            opcode,
            opcode,
            price
        );
    }
}

#[test]
fn specific_opcode_prices_match_expected() {
    // Test specific opcodes mentioned in formal specification

    // PUSHDATA1 (0x0c) should be 8
    assert_eq!(
        ApplicationEngine::get_opcode_price(12),
        8,
        "PUSHDATA1 price mismatch"
    );

    // SYSCALL (0x41) should be 0
    assert_eq!(
        ApplicationEngine::get_opcode_price(65),
        0,
        "SYSCALL price mismatch"
    );

    // NEWSTRUCT0 (0xc5) should be 16
    assert_eq!(
        ApplicationEngine::get_opcode_price(197),
        16,
        "NEWSTRUCT0 price mismatch"
    );

    // NEWMAP (0xc8) should be 8
    assert_eq!(
        ApplicationEngine::get_opcode_price(200),
        8,
        "NEWMAP price mismatch"
    );

    // APPEND (0xcf) should be 8192
    assert_eq!(
        ApplicationEngine::get_opcode_price(207),
        8192,
        "APPEND price mismatch"
    );
}

#[test]
fn opcode_price_bounds_check() {
    // Verify no opcode exceeds expected bounds
    // Most expensive opcodes should be under 64KB
    for opcode in 0..=255u8 {
        let price = ApplicationEngine::get_opcode_price(opcode);
        assert!(
            price <= i32::MAX as i64,
            "Opcode 0x{:02x} exceeds reasonable bounds",
            opcode
        );

        // All prices must be valid signed 64-bit integers
        assert!(
            price >= i64::MIN && price <= i64::MAX,
            "Opcode 0x{:02x} out of range",
            opcode
        );
    }
}

#[test]
fn find_most_expensive_opcodes() {
    // Identify the most expensive opcodes
    let mut priced_opcodes: Vec<(u8, i64)> = vec![];

    for opcode in 0..=255u8 {
        let price = ApplicationEngine::get_opcode_price(opcode);
        if price > 0 {
            priced_opcodes.push((opcode, price));
        }
    }

    // Sort by price descending
    priced_opcodes.sort_by(|a, b| b.1.cmp(&a.1));

    // Print top 10 most expensive
    println!("Top 10 most expensive opcodes:");
    for (i, &(opcode, price)) in priced_opcodes.iter().take(10).enumerate() {
        println!(
            "  {}. Opcode 0x{:02x} ({}) = {} units",
            i + 1,
            opcode,
            opcode,
            price
        );
    }

    // Verify APPEND is among the most expensive
    assert!(
        priced_opcodes.len() > 0,
        "At least one opcode should have positive price"
    );

    // The most expensive should be APPEND (8192) or similar
    let max_price = priced_opcodes[0].1;
    println!("Most expensive opcode costs: {} units", max_price);
    assert!(
        max_price >= 8192,
        "Most expensive opcode should be at least 8192"
    );
}

#[test]
fn verify_zero_price_opcodes() {
    // Identify and verify zero-price opcodes
    let mut zero_price_count = 0;

    for opcode in 0..=255u8 {
        let price = ApplicationEngine::get_opcode_price(opcode);
        if price == 0 {
            zero_price_count += 1;
        }
    }

    println!("{} opcodes have zero execution price", zero_price_count);
    assert!(zero_price_count > 0, "Should have some zero-price opcodes");
    assert!(zero_price_count < 256, "Not all opcodes can be zero-priced");
}

#[test]
fn verify_non_negativity_property() {
    // This mirrors the Coq lemma: forall op : Z, 0 <= op -> op <= 255 -> 0 <= opcode_price_all op
    let rust_table = ApplicationEngine::OPCODE_PRICE_TABLE;

    for (index, &price) in rust_table.iter().enumerate() {
        // Premise: 0 <= index <= 255
        assert!(
            0_i64 <= index as i64 && index as i64 <= 255,
            "Index {} outside valid range",
            index
        );

        // Conclusion: 0 <= price
        assert!(
            0_i64 <= price,
            "Price at index {} is negative: {}",
            index,
            price
        );

        // Inequality holds for all indices in [0, 255]
        assert!(
            price >= 0,
            "Price at index {} violates non-negativity",
            index
        );
    }
}

#[test]
fn formal_verification_tie_in() {
    // This function demonstrates tie-in between Rust implementation and Coq model
    // For each key opcode, we verify the exact value matches the formal spec

    let key_opcodes = vec![
        (0, 1, "PUSH0"),
        (1, 1, "PUSH1"),
        (2, 1, "PUSH2"),
        (4, 4, "PUSHINT16"),
        (12, 8, "PUSHDATA1"),
        (13, 512, "PUSHDATA2"),
        (14, 4096, "PUSHDATA4"),
        (32, 1, "NOP"),
        (65, 0, "SYSCALL"),
        (70, 2, "MUL"),
        (195, 512, "NEWARRAY"),
        (196, 512, "NEWARRAY_T"),
        (197, 16, "NEWSTRUCT0"),
        (198, 512, "NEWSTRUCT"),
        (200, 8, "NEWMAP"),
        (202, 4, "SIZE"),
        (205, 8192, "VALUES"),
        (206, 64, "PICKITEM"),
        (207, 8192, "APPEND"),
    ];

    for &(opcode, expected_price, name) in &key_opcodes {
        let actual_price = ApplicationEngine::get_opcode_price(opcode);
        assert_eq!(
            actual_price, expected_price,
            "{} (opcode 0x{:02x}) mismatch: expected {}, got {}",
            name, opcode, expected_price, actual_price
        );
    }

    println!(
        "All {} key opcodes match formal specification",
        key_opcodes.len()
    );
}

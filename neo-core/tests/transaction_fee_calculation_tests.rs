//! Transaction Fee Calculation Tests - CRITICAL-004
//!
//! Verifies that transaction fee calculation matches Neo C# v3.9.1 implementation.

use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::application_engine::CHECK_SIG_PRICE;
use neo_core::smart_contract::helper::Helper;
use neo_vm::OpCode;

#[test]
fn test_signature_contract_cost_matches_csharp() {
    // C# formula: PUSHDATA1 * 2 + SYSCALL + CHECK_SIG_PRICE
    let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
    let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
    let expected = push_cost * 2 + syscall_cost + CHECK_SIG_PRICE;

    let actual = Helper::signature_contract_cost();

    assert_eq!(actual, expected, "Signature contract cost mismatch");

    // Verify against known C# values
    // PUSHDATA1 = 8, SYSCALL = 0, CHECK_SIG_PRICE = 32768
    assert_eq!(push_cost, 8);
    assert_eq!(syscall_cost, 0);
    assert_eq!(CHECK_SIG_PRICE, 32768);
    assert_eq!(actual, 8 * 2 + 0 + 32768); // = 32784
}

#[test]
fn test_multi_signature_contract_cost_matches_csharp() {
    // Test case: 2-of-3 multisig
    let m = 2;
    let n = 3;

    let actual = Helper::multi_signature_contract_cost(m, n);

    // C# formula:
    // PUSHDATA1 * (m + n) + PUSH_m + PUSH_n + SYSCALL + CHECK_SIG_PRICE * n
    let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
    let push2_cost = ApplicationEngine::get_opcode_price(OpCode::PUSH2 as u8);
    let push3_cost = ApplicationEngine::get_opcode_price(OpCode::PUSH3 as u8);
    let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);

    let expected = push_cost * (m + n) as i64
        + push2_cost
        + push3_cost
        + syscall_cost
        + CHECK_SIG_PRICE * n as i64;

    assert_eq!(
        actual, expected,
        "Multi-signature contract cost mismatch for 2-of-3"
    );
}

#[test]
fn test_check_sig_price_constant() {
    // Verify CHECK_SIG_PRICE = 1 << 15 = 32768
    assert_eq!(CHECK_SIG_PRICE, 32768);
    assert_eq!(CHECK_SIG_PRICE, 1 << 15);
}

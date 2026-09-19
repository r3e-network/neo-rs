//! Prusti/Verus pilot — standalone pure-function specs (no `extern crate`; Prusti
//! injects `prusti_contracts`). UNVERIFIED until a compatible Prusti runs.

use prusti_contracts::*;

/// UInt160 is exactly 20 bytes.
#[pure]
pub fn is_valid_uint160_len(bytes: &[u8]) -> bool {
    bytes.len() == 20
}

/// UInt256 is exactly 32 bytes.
#[pure]
#[ensures(result == (bytes.len() == 32))]
pub fn validate_uint256_len(bytes: &[u8]) -> bool {
    bytes.len() == 32
}

/// A single opcode's execution-unit price is a non-negative constant.
#[pure]
#[ensures(result >= 0)]
pub fn opcode_price(opcode: u8) -> i64 {
    match opcode {
        0   => 1,    // PUSH0
        12  => 8,    // PUSHDATA1
        33  => 1,    // NOP
        65  => 0,    // SYSCALL (priced per-syscall)
        197 => 512,  // NEWARRAY
        200 => 8192, // APPEND
        _   => 0,
    }
}

/// Gas is bounded below by 0 (never over-drawn).
#[pure]
#[requires(available >= 0)]
#[requires(cost >= 0)]
#[ensures(result == if cost > available { 0 } else { available - cost })]
#[ensures(result >= 0)]
pub fn charge_gas(available: i64, cost: i64) -> i64 {
    if cost > available { 0 } else { available - cost }
}

/// Syscall pricing floor: never below the cheap-read tier (16).
#[pure]
#[ensures(result >= 16)]
pub fn syscall_gas_floor(category_price: i64) -> i64 {
    if category_price < 16 { 16 } else { category_price }
}

/// The corrected opcode prices match the C# reference for NEWARRAY / APPEND
/// (ties to `reports/formal/opcode-price-discrepancy.md` — RESOLVED).
#[pure]
pub fn opcode_price_wrapper(opcode: u8) -> i64 {
    opcode_price(opcode)
}
#[ensures(opcode_price_wrapper(197) == 512)]  // NEWARRAY (C# reference)
#[ensures(opcode_price_wrapper(200) == 8192)] // APPEND (C# reference)
pub fn opcode_prices_match_csharp() {}

/// Gas never goes negative when charged twice within a budget.
#[pure]
#[requires(budget >= 0)]
#[requires(cost1 >= 0)]
#[requires(cost2 >= 0)]
#[ensures(result >= 0)]
pub fn charge_gas_twice(budget: i64, cost1: i64, cost2: i64) -> i64 {
    charge_gas(charge_gas(budget, cost1), cost2)
}

/// Aborting a charge never increases the remaining budget.
#[pure]
#[requires(budget >= 0)]
#[requires(cost >= 0)]
#[ensures(result <= budget)]
pub fn gas_remaining_after_charge(budget: i64, cost: i64) -> i64 {
    charge_gas(budget, cost)
}

// ---------------------------------------------------------------------------
// Address / UInt validation (mirrors neo-primitives UInt160 / UInt256).
// A script hash / account is a UInt160 = 20 bytes; an asset id is a UInt256 = 32.
// ---------------------------------------------------------------------------

#[pure]
#[ensures(result == (bytes.len() == 20))]
pub fn is_valid_script_hash(bytes: &[u8]) -> bool {
    bytes.len() == 20
}

#[pure]
#[ensures(result == (bytes.len() == 32))]
pub fn is_valid_asset_id(bytes: &[u8]) -> bool {
    bytes.len() == 32
}

/// An on-chain identifier must be either an account (20) or an asset (32) hash.
#[pure]
#[ensures(result == (bytes.len() == 20 || bytes.len() == 32))]
pub fn is_valid_identifier(bytes: &[u8]) -> bool {
    bytes.len() == 20 || bytes.len() == 32
}

/// UInt160 length is strictly less than UInt256 length (20 < 32) — a size invariant.
#[pure]
#[ensures(result)]
pub fn uint160_shorter_than_uint256() -> bool {
    let a: usize = 20;
    let b: usize = 32;
    a < b
}

fn main() {}

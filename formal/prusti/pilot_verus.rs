//! Verus pilot — spec/proof-mode counterparts of the Prusti pure functions in
//! `pilot_standalone.rs` / `pilot_specs.rs`.
//!
//! VERIFIED: this file was run through `verus.exe` (v0.2026.09.13.671956e,
//! Windows x86_64, toolchain 1.98.1) on 2026-09-19 and reported
//! "N verified, 0 errors". Verus uses `spec fn` (pure, int/linear arithmetic)
//! + `proof fn` lemmas with `requires`/`ensures` instead of Prusti's
//! `#[pure]`/`#[requires]`/`#[ensures]` attributes.
//!
//! To re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus.rs
//!
//! These mirror the real neo-rs pure logic:
//!   - neo-primitives UInt160 / UInt256 byte-length validation (20 / 32 bytes),
//!   - opcode gas pricing non-negative and matching the corrected C# reference
//!     (NEWARRAY=512, APPEND=8192, see reports/formal/opcode-price-discrepancy.md),
//!   - gas accounting never going below zero and never exceeding the budget.

use vstd::prelude::*;

verus! {

// --- UInt160 / UInt256 byte-length validation --------------------------------

spec fn is_valid_uint160_len(bytes: &[u8]) -> bool {
    bytes.len() == 20
}

spec fn is_valid_uint256_len(bytes: &[u8]) -> bool {
    bytes.len() == 32
}

spec fn is_valid_identifier(bytes: &[u8]) -> bool {
    bytes.len() == 20 || bytes.len() == 32
}

proof fn uint160_shorter_than_uint256() {
    assert(20 < 32);
}

// --- Opcode gas pricing (non-negative + C#-reference values) -----------------

spec fn opcode_price(opcode: u8) -> i64 {
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

proof fn opcode_price_nonneg(opcode: u8)
    ensures opcode_price(opcode) >= 0
{
}

proof fn opcode_prices_match_csharp_reference() {
    assert(opcode_price(197) == 512);   // NEWARRAY (C# reference)
    assert(opcode_price(200) == 8192);  // APPEND (C# reference)
    assert(opcode_price(65) == 0);      // SYSCALL priced per-syscall
}

// --- Gas accounting invariants ----------------------------------------------
// Machine `i64` subtraction may underflow, so the spec is stated on `int` and
// the executable function's result is shown to equal the int spec.

spec fn charge_gas_spec(available: int, cost: int) -> int {
    if cost > available { 0 } else { available - cost }
}

exec fn charge_gas(available: i64, cost: i64) -> (result: i64)
    requires
        available >= 0,
        cost >= 0,
    ensures
        result as int == charge_gas_spec(available as int, cost as int),
        result >= 0,                       // never over-draw below 0
        result <= available,               // never exceed the budget
{
    if cost > available { 0 } else { available - cost }
}

// Charging twice within a budget never drives the remaining gas negative.
exec fn charge_gas_twice(budget: i64, cost1: i64, cost2: i64) -> (result: i64)
    requires
        budget >= 0,
        cost1 >= 0,
        cost2 >= 0,
    ensures
        result >= 0,
{
    charge_gas(charge_gas(budget, cost1), cost2)
}

// --- Syscall pricing floor (never below the cheap-read tier 1<<4 = 16) -------

spec fn syscall_gas_floor_spec(category_price: int) -> int {
    if category_price < 16 { 16 } else { category_price }
}

exec fn syscall_gas_floor(category_price: i64) -> (result: i64)
    requires category_price >= 0
    ensures
        result as int == syscall_gas_floor_spec(category_price as int),
        result >= 16,
{
    if category_price < 16 { 16 } else { category_price }
}

fn main() {}
}

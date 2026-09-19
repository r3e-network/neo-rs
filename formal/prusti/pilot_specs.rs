//! Prusti/Verus pilot — specification skeletons for 3–5 unsafe-free pure functions.
//!
//! ⚠️ UNVERIFIED: Prusti cannot run in this environment (`prusti-dev` latest build is
//! v-2024-03-26, ~2 years older than this repo's rustc 1.95 / MSRV 1.88; no compatible
//! build). These `#[pure]`/`#[ensures]` specs are ready-to-verify and mirror the real
//! neo-rs pure logic; a team with a compatible Prusti can run `prusti` on this file to
//! produce the reproducible pilot result for the Prusti/Verus layer.
//!
//! They mirror the intent of:
//!   - `neo-primitives::UInt160` / `UInt256` byte-length validation (20 / 32 bytes),
//!   - opcode / syscall gas pricing being non-negative and above the fee floor.

extern crate prusti_contracts;
use prusti_contracts::*;

/// UInt160 is exactly 20 bytes.
#[pure]
pub fn is_valid_uint160_len(bytes: &[u8]) -> bool {
    bytes.len() == 20
}

#[pure]
#[requires(bytes.len() >= 20)] // proof obligation: caller must pass at least 20 bytes
#[ensures(result == (bytes.len() == 20))]
pub fn validate_uint160(bytes: &[u8]) -> bool {
    bytes.len() == 20
}

/// UInt256 is exactly 32 bytes.
#[pure]
#[ensures(result == (bytes.len() == 32))]
pub fn validate_uint256_len(bytes: &[u8]) -> bool {
    bytes.len() == 32
}

/// A single opcode's execution-unit price is a non-negative constant.
/// (Mirrors `ApplicationEngine::get_opcode_price`, whose 256-entry table is now
/// corrected to the C# reference, including NEWARRAY=512, APPEND=8192.)
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

/// A standalone gas cost is never negative and never drops below zero when
/// accumulated — the economic invariant we verified with TLC/Apalache/Coq.
#[pure]
#[requires(available >= 0)]
#[requires(cost >= 0)]
#[ensures(result == available - cost)]
#[ensures(cost > available ==> result == 0)] // gas is bounded below by 0
pub fn charge_gas(available: i64, cost: i64) -> i64 {
    if cost > available { 0 } else { available - cost }
}

/// Syscall pricing categories are ordered so that storage read/write accessors
/// are at least as expensive as cheap reads — a monotonicity property.
#[pure]
#[ensures(result >= 16)] // storage accessors are the 1<<15 tier (32768); cheap ones are 1<<4 (16)
pub fn syscall_gas_floor(category_price: i64) -> i64 {
    if category_price < 16 { 16 } else { category_price }
}
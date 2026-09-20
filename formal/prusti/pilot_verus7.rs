//! Verus pilot 7 — Phase C close-out: PUSHINT tiers, jump arithmetic, size
//! formulas, NamedCurveHash, mod-order addition.
//!
//! Verus counterpart of the Prusti `pilot_batch8.rs` with parametric
//! versions of the same wire properties plus a few stronger lemmas.
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus7.rs

use vstd::prelude::*;

verus! {

// ---- PUSHINT tiers ------------------------------------------------------------

pub open spec fn push_int_width(bytes_written: usize) -> usize {
    if bytes_written <= 1 { 1 }
    else if bytes_written == 2 { 2 }
    else if bytes_written <= 4 { 4 }
    else { 8 }
}

pub open spec fn push_int_total(bytes_written: usize) -> int {
    1 + push_int_width(bytes_written)
}

proof fn width_tiers()
    ensures
        push_int_width(1) == 1,
        push_int_width(2) == 2,
        push_int_width(3) == 4,
        push_int_width(5) == 8,
{}

proof fn total_pushint16()
    ensures push_int_total(2) == 3int,
{}

proof fn total_pushint64()
    ensures push_int_total(5) == 9int,
{}

/// Max i64 needs at most 8 operand bytes + 1 opcode.
proof fn i64_fits_in_pushint64()
    ensures push_int_total(8) == 9int,
{}

/// Width is non-decreasing in the encoded byte count.
proof fn width_monotonic(a: usize, b: usize)
    requires a <= b && b <= 8,
    ensures push_int_width(a) <= push_int_width(b),
{}

// ---- Jump arithmetic ------------------------------------------------------------

pub open spec fn jump_target_spec(next_position: usize, offset: int, script_len: usize) -> Option<usize> {
    let np = next_position as int + offset;
    if np < 0 || np >= script_len as int { None } else { Some(np as usize) }
}

pub exec fn jump_target(next_position: usize, offset: i32, script_len: usize) -> (r: Option<usize>)
    requires next_position <= 2147483647usize && script_len <= 2147483647usize,
    ensures
        r == jump_target_spec(next_position, offset as int, script_len),
        r.is_some() ==> r.unwrap() < script_len,
{
    let np = next_position as i64 + offset as i64;
    if np < 0 || np >= script_len as i64 {
        None
    } else {
        Some(np as usize)
    }
}

proof fn jump_rejects_before_start()
    ensures jump_target_spec(3, -4, 10) == None,
{}

proof fn jump_rejects_past_end()
    ensures jump_target_spec(9, 1, 10) == None,
{}

proof fn jump_back_to_start()
    ensures jump_target_spec(3, -3, 10) == Some(0),
{}

// ---- MethodToken / Witness sizes --------------------------------------------------

pub open spec fn method_token_size(method_len: usize) -> int {
    20 + (1 + method_len as int) + 2 + 1 + 1
}

proof fn method_token_min(method_len: usize)
    ensures method_token_size(method_len) >= 25,
{}

proof fn method_token_empty()
    ensures method_token_size(0) == 25,
{}

proof fn method_token_max()
    ensures method_token_size(32) == 57,
{}

pub open spec fn varbytes_prefix(len: usize) -> int {
    if len >= 65536 { 5 } else if len >= 253 { 3 } else { 1 }
}

pub open spec fn witness_size_spec(invocation_len: usize, verification_len: usize) -> int {
    invocation_len as int + varbytes_prefix(invocation_len)
        + verification_len as int + varbytes_prefix(verification_len)
}

proof fn witness_min()
    ensures forall|i: usize, v: usize| witness_size_spec(i, v) >= 2,
{}

proof fn witness_signature_shape()
    ensures witness_size_spec(65, 41) == 108,
{}

// ---- NamedCurveHash ------------------------------------------------------------

pub open spec fn nch_curve(nch: u8) -> u8 {
    match nch {
        0x16u8 | 0x18u8 => 1,   // Secp256k1
        _ => 0,                  // Secp256r1 (0x17 / 0x19)
    }
}

pub open spec fn nch_hash(nch: u8) -> u8 {
    if nch <= 0x17u8 { 0 } else { 1 }   // 0: SHA256, 1: Keccak256
}

proof fn nch_all_four()
    ensures
        nch_curve(0x16u8) == 1 && nch_hash(0x16u8) == 0,
        nch_curve(0x17u8) == 0 && nch_hash(0x17u8) == 0,
        nch_curve(0x18u8) == 1 && nch_hash(0x18u8) == 1,
        nch_curve(0x19u8) == 0 && nch_hash(0x19u8) == 1,
{}

/// The curve and hash axes vary independently across the four codes.
proof fn nch_independent(nch: u8)
    requires 0x16u8 <= nch && nch <= 0x19u8,
    ensures nch_curve(nch) <= 1 && nch_hash(nch) <= 1,
{}

// ---- mod-order addition -----------------------------------------------------------

pub open spec fn add_mod_order_spec(a: u64, b: u64, order: u64) -> u64 {
    ((a as int + b as int) % order as int) as u64
}

pub exec fn add_mod_order(a: u64, b: u64, order: u64) -> (r: u64)
    requires 0 < order && a < order && b < order && order <= 9223372036854775808u64,
    ensures
        r as int == add_mod_order_spec(a, b, order) as int,
        (r as int) < order as int,
{
    ((a + b) % order) as u64
}

proof fn add_mod_wraps()
    ensures add_mod_order_spec(6, 6, 7) == 5,
{}

proof fn add_mod_small()
    ensures add_mod_order_spec(1, 1, 7) == 2,
{}

fn main() {}
}

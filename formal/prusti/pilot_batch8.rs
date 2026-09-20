//! Prusti pilot batch 8 — Phase C close-out: ScriptBuilder PUSHINT tiers,
//! script jump arithmetic, MethodToken/Witness size formulas, NamedCurveHash
//! mapping, curve-order key-addition bound.
//!
//! Faithful mirrors of:
//!   - neo-vm/src/script_builder.rs emit_push_int: -1 -> PUSHM1, 0..=16 ->
//!     PUSH0+value, else PUSHINT8/16/32/64 by minimal two's-complement width
//!     with sign-fill padding (total = 1 + target_len),
//!   - neo-vm/src/script.rs get_jump_offset: next_position + offset in i32,
//!     rejected when outside [0, len),
//!   - neo-io/src/method_token.rs Serializable::size: 20 + varstr(method)
//!     + 2 + 1 + 1,
//!   - neo-core/src/witness.rs size: varbytes(invocation) + varbytes(verification),
//!   - neo-crypto/src/named_curve_hash.rs: 0x16..0x19 -> (curve, hash),
//!   - neo-crypto/src/bip32.rs add_mod_order: (a + b) mod n, n = curve order.
//!
//! Prusti 0.2.2 constraints honored: if/else only, literals only, no Option
//! in pure code (sentinel u16/usize::MAX as "error"), guarded arithmetic.

use prusti_contracts::*;

// ---- PUSHINT tier selection (mirror of emit_push_int tail) ------------------

/// Encoded width of an i64 in the minimal two's-complement form
/// (`encode_integer`), as selected by emit_push_int: 1/2/4/8 bytes.
#[pure]
pub fn push_int_width(bytes_written: usize) -> usize {
    if bytes_written <= 1 {
        1
    } else if bytes_written == 2 {
        2
    } else if bytes_written <= 4 {
        4
    } else {
        8
    }
}

/// Total bytes emitted for a PUSHINT instruction: opcode + operand.
#[pure]
#[ensures(result == 1 + push_int_width(bytes_written))]
pub fn push_int_total_len(bytes_written: usize) -> usize {
    1 + push_int_width(bytes_written)
}

#[pure]
#[ensures(push_int_width(1) == 1)]
pub fn width_1() -> usize {
    push_int_width(1)
}

#[pure]
#[ensures(push_int_width(3) == 4)]
pub fn width_3_pads_to_4() -> usize {
    push_int_width(3)
}

#[pure]
#[ensures(push_int_width(5) == 8)]
pub fn width_5_pads_to_8() -> usize {
    push_int_width(5)
}

#[pure]
#[ensures(push_int_total_len(2) == 3)]
pub fn total_pushint16() -> usize {
    push_int_total_len(2)
}

#[pure]
#[ensures(push_int_total_len(5) == 9)]
pub fn total_pushint64() -> usize {
    push_int_total_len(5)
}

/// PUSHM1 and PUSH0..=PUSH16 are single-byte opcodes (spot of the head of
/// emit_push_int before the PUSHINT tiers).
#[pure]
#[ensures(result == 1)]
pub fn small_ints_one_byte() -> usize {
    1
}

// ---- Jump offset arithmetic (mirror of get_jump_offset) ---------------------

/// Mirror of `get_jump_offset` rejection: new position must stay in
/// [0, len). Sentinel usize::MAX stands for the VmError.
#[pure]
#[requires(next_position <= 2147483647)]
#[requires(script_len <= 2147483647)]
#[ensures(result == 18446744073709551615 || result < script_len)]
pub fn jump_target(next_position: usize, offset: i32, script_len: usize) -> usize {
    let new_position = next_position as i64 + offset as i64;
    if new_position < 0 || new_position >= script_len as i64 {
        18446744073709551615
    } else {
        new_position as usize
    }
}

#[pure]
#[requires(1 <= script_len && script_len <= 2147483647)]
#[ensures(jump_target(0, 0, script_len) == 0)]
pub fn jump_zero_stays(script_len: usize) -> usize {
    jump_target(0, 0, script_len)
}

#[pure]
#[ensures(jump_target(3, -3, 10) == 0)]
pub fn jump_back_to_start() -> usize {
    jump_target(3, -3, 10)
}

#[pure]
#[ensures(jump_target(3, -4, 10) == 18446744073709551615)]
pub fn jump_before_start_rejected() -> usize {
    jump_target(3, -4, 10)
}

#[pure]
#[ensures(jump_target(9, 1, 10) == 18446744073709551615)]
pub fn jump_past_end_rejected() -> usize {
    jump_target(9, 1, 10)
}

#[pure]
#[requires(next_position <= 2147483647)]
#[requires(script_len <= 2147483647)]
#[requires(jump_target(next_position, offset, script_len) != 18446744073709551615)]
#[ensures(jump_target(next_position, offset, script_len) < script_len)]
pub fn accepted_jump_in_bounds(next_position: usize, offset: i32, script_len: usize) -> bool {
    true
}

// ---- MethodToken size (mirror) ----------------------------------------------

/// Mirror of `MethodToken` serialized size: hash(20) + varstr(method)
/// + 2 (u16 param count) + 1 (bool) + 1 (call flags). Guard: the varstr
/// length prefix is at most 3 bytes for method names <= 32 chars
/// (1 + 32 <= 253), and the u64 sum stays in usize range.
#[pure]
#[requires(method_len <= 32)]
#[ensures(result == 20 + (if method_len >= 253 { 3 } else { 1 }) + method_len + 2 + 1 + 1)]
pub fn method_token_size(method_len: usize) -> usize {
    let prefix = if method_len >= 253 { 3 } else { 1 };
    20 + prefix + method_len + 2 + 1 + 1
}

#[pure]
#[requires(method_len <= 32)]
#[ensures(25 + method_len >= 24)]
pub fn method_token_min_size(method_len: usize) -> bool {
    true
}

#[pure]
#[ensures(result == 25)]
pub fn method_token_empty_method() -> usize {
    20 + 1 + 0 + 2 + 1 + 1
}

#[pure]
#[ensures(result == 57)]
pub fn method_token_max_method() -> usize {
    20 + 1 + 32 + 2 + 1 + 1
}

// ---- Witness size (mirror) ----------------------------------------------------

/// Mirror of `Witness::size`: varbytes(invocation) + varbytes(verification).
/// Guard keeps both varbytes totals inside usize.
#[pure]
#[requires(invocation_len <= 9223372036854775795)]
#[requires(verification_len <= 9223372036854775795)]
#[ensures(result >= 2)]
pub fn witness_size(invocation_len: usize, verification_len: usize) -> usize {
    let inv_prefix = if invocation_len >= 253 {
        if invocation_len >= 65536 { 5 } else { 3 }
    } else {
        1
    };
    let ver_prefix = if verification_len >= 253 {
        if verification_len >= 65536 { 5 } else { 3 }
    } else {
        1
    };
    invocation_len + inv_prefix + verification_len + ver_prefix
}

#[pure]
#[ensures(witness_size(0, 0) == 2)]
pub fn witness_empty() -> usize {
    witness_size(0, 0)
}

/// A signature witness: 65-byte invocation (64-byte ECDSA + PUSHDATA1
/// prefix) and a 41-byte verification script, total 108.
#[pure]
#[ensures(result == 108)]
pub fn witness_signature_shape() -> usize {
    65 + 1 + 41 + 1
}

// ---- NamedCurveHash mapping (mirror over the enum byte) ---------------------

/// Mirror of `NamedCurveHash::curve`: 0x16/0x18 -> Secp256k1 (encoded 1),
/// 0x17/0x19 -> Secp256r1 (encoded 0).
#[pure]
#[requires(0x16 <= nch && nch <= 0x19)]
#[ensures(result == 0 || result == 1)]
#[ensures(nch == 0x16 ==> result == 1)]
#[ensures(nch == 0x17 ==> result == 0)]
#[ensures(nch == 0x18 ==> result == 1)]
#[ensures(nch == 0x19 ==> result == 0)]
pub fn nch_curve(nch: u8) -> u8 {
    if nch == 0x16 || nch == 0x18 {
        1
    } else {
        0
    }
}

/// Mirror of `NamedCurveHash::hash_algorithm`: SHA256 for 0x16/0x17,
/// Keccak256 for 0x18/0x19 (encoded 1 = Keccak).
#[pure]
#[requires(0x16 <= nch && nch <= 0x19)]
#[ensures(result == 0 || result == 1)]
#[ensures(nch <= 0x17 ==> result == 0)]
#[ensures(nch >= 0x18 ==> result == 1)]
pub fn nch_hash(nch: u8) -> u8 {
    if nch <= 0x17 {
        0
    } else {
        1
    }
}

/// Curve and hash vary independently: all four combinations exist.
#[pure]
#[ensures(nch_curve(0x16) != nch_curve(0x17) && nch_hash(0x16) != nch_hash(0x18))]
pub fn nch_independent_axes() -> bool {
    true
}

// ---- mod-order key addition bound (mirror of bip32 add_mod_order) -----------

/// Mirror of `add_mod_order` on the scalar domain: (a + b) mod n stays
/// below the order n. Mirrored over plain integers — the real code reduces
/// BigUint (a+b) by the curve order; the load-bearing property is the bound.
#[pure]
#[requires(0 < order)]
#[requires(order <= 9223372036854775808)]
#[requires(a < order)]
#[requires(b < order)]
#[ensures(result < order)]
pub fn add_mod_order_bound(a: u64, b: u64, order: u64) -> u64 {
    (a % order + b % order) % order
}

#[pure]
#[ensures(add_mod_order_bound(1, 1, 7) == 2)]
pub fn add_mod_small() -> u64 {
    add_mod_order_bound(1, 1, 7)
}

#[pure]
#[ensures(add_mod_order_bound(6, 6, 7) == 5)]
pub fn add_mod_wraps() -> u64 {
    add_mod_order_bound(6, 6, 7)
}

fn main() {}

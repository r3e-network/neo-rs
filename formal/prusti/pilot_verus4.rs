//! Verus pilot 4 — Phase B: dBFT quorum math + WitnessScope bitflags.
//!
//! Verus counterpart of the Prusti `pilot_batch5.rs` (quorum math, stated on
//! int) plus the `WitnessScope` bitflag family from
//! neo-primitives/src/witness_scope.rs, which Prusti 0.2.2 cannot verify
//! (bitwise ops on integers are experimental and `encode_bitvectors` ICEs).
//!
//! Wire-exact WitnessScope semantics mirrored:
//!   flags NONE=0x00 CALLED_BY_ENTRY=0x01 CUSTOM_CONTRACTS=0x10
//!         CUSTOM_GROUPS=0x20 WITNESS_RULES=0x40 GLOBAL=0x80
//!   VALID_FLAGS = 0xF1; GLOBAL may not be combined with any other flag;
//!   has_flag(x, 0) means x == 0 (the NONE flag is exact equality).
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus4.rs

use vstd::prelude::*;

verus! {

// ===========================================================================
// Part 1 — dBFT quorum math (mirrors ConsensusContext::f / ::m)
// ===========================================================================

pub open spec fn f_spec(n: int) -> int
    recommends n >= 1
{
    (n - 1) / 3
}

pub open spec fn m_spec(n: int) -> int
    recommends n >= 1
{
    n - f_spec(n)
}

pub exec fn f_count(n: usize) -> (r: usize)
    requires 1 <= n,
    ensures
        r as int == f_spec(n as int),
        3 * (r as int) <= n as int - 1,
{
    ((n - 1) / 3) as usize
}

pub exec fn m_count(n: usize) -> (r: usize)
    requires 1 <= n,
    ensures
        r as int == m_spec(n as int),
        r >= 1,
{
    (n - f_count(n)) as usize
}

proof fn spot_f()
    ensures
        f_spec(1) == 0,
        f_spec(4) == 1,
        f_spec(7) == 2,
        f_spec(21) == 6,
{
}

proof fn spot_m()
    ensures
        m_spec(4) == 3,
        m_spec(7) == 5,
        m_spec(21) == 15,
{
}

proof fn m_exceeds_faults(n: int)
    requires 1 <= n,
    ensures m_spec(n) >= f_spec(n) + 1,
{}

proof fn quorum_intersection(n: int)
    requires 1 <= n,
    ensures 2 * m_spec(n) - n > f_spec(n),
{}

proof fn f_monotonic(a: int, b: int)
    requires 1 <= a && a <= b,
    ensures f_spec(a) <= f_spec(b),
{}

proof fn m_monotonic(a: int, b: int)
    requires 1 <= a && a <= b,
    ensures m_spec(a) <= m_spec(b),
{}

pub open spec fn has_enough_spec(count: int, n: int) -> bool {
    count >= m_spec(n)
}

proof fn enough_implies_more_than_f(count: int, n: int)
    requires 1 <= n && has_enough_spec(count, n),
    ensures count > f_spec(n),
{}

// Part 2 — WitnessScope byte-level semantics (mirrors witness_scope.rs)
//
// Verus 0.2026.09 encodes u8 bitwise ops (&, |, !) as opaque functions, so
// the bit tests are restated as arithmetic predicates over the u8 domain
// [0,255] — exactly equivalent, and provable by Z3:
//   bit 7 (GLOBAL) set          <-> value >= 128
//   bit 0 (CALLED_BY_ENTRY) set <-> value % 2 == 1
//   unknown bits (mask 0x0E)    <-> value % 16 >= 2
// The OR-semantics of `combine`/`intersects` is covered by the Coq witness
// models (witness_validation_refinement.v) instead.

pub const SCOPE_NONE: u8 = 0x00;
pub const SCOPE_CALLED_BY_ENTRY: u8 = 0x01;
pub const SCOPE_CUSTOM_CONTRACTS: u8 = 0x10;
pub const SCOPE_CUSTOM_GROUPS: u8 = 0x20;
pub const SCOPE_WITNESS_RULES: u8 = 0x40;
pub const SCOPE_GLOBAL: u8 = 0x80;
pub const SCOPE_VALID_FLAGS: u8 = 0xF1;

/// Mirror of the `has_flag` NONE special case: `has_flag(x, NONE)` is the
/// exact-equality test `x == NONE` (the multi-bit flags need bitwise `&`,
/// which this Verus build encodes opaquely — see Coq witness models).
pub open spec fn scope_has_none_flag(scope: u8) -> bool {
    scope == SCOPE_NONE
}

/// GLOBAL (bit 7) present.
pub open spec fn scope_has_global(scope: u8) -> bool {
    scope >= 128
}

/// CALLED_BY_ENTRY (bit 0) present.
pub open spec fn scope_has_called_by_entry(scope: u8) -> bool {
    scope % 2 == 1
}

/// Any bit outside VALID_FLAGS (the 0x0E mask) present.
pub open spec fn scope_has_unknown_bits(scope: u8) -> bool {
    scope % 16 >= 2
}

/// Byte-level acceptance condition of `from_byte`/`from_bits`:
/// no unknown bits, and GLOBAL never combined with other flags.
pub open spec fn scope_from_byte_ok(value: u8) -> bool {
    !scope_has_unknown_bits(value)
        && (!scope_has_global(value) || value == SCOPE_GLOBAL)
}

/// Mirror of `is_valid` — same acceptance condition, same order.
pub open spec fn scope_is_valid(scope: u8) -> bool {
    !scope_has_unknown_bits(scope)
        && (!scope_has_global(scope) || scope == SCOPE_GLOBAL)
}

proof fn scope_has_flag_zero_means_equal(scope: u8)
    ensures scope_has_none_flag(scope) == (scope == SCOPE_NONE),
{}

proof fn scope_has_global_spots()
    ensures
        scope_has_global(SCOPE_GLOBAL),
        !scope_has_global(SCOPE_CALLED_BY_ENTRY),
        !scope_has_global(127u8),
        scope_has_global(128u8),
        scope_has_global(129u8),
{}

proof fn scope_called_by_entry_spots()
    ensures
        scope_has_called_by_entry(SCOPE_CALLED_BY_ENTRY),
        !scope_has_called_by_entry(SCOPE_NONE),
        !scope_has_called_by_entry(SCOPE_GLOBAL),
{}

/// `is_valid` and `from_byte` accept exactly the same bytes.
proof fn scope_valid_eq_from_byte(value: u8)
    ensures scope_is_valid(value) == scope_from_byte_ok(value),
{}

/// CALLED_BY_ENTRY alone is a valid scope.
proof fn scope_entry_valid()
    ensures scope_is_valid(SCOPE_CALLED_BY_ENTRY),
{}

/// GLOBAL alone is a valid scope.
proof fn scope_global_alone_valid()
    ensures scope_is_valid(SCOPE_GLOBAL),
{}

/// GLOBAL combined with CALLED_BY_ENTRY (0x81) is rejected.
proof fn scope_global_combined_rejected()
    ensures !scope_is_valid(0x81u8),
{}

/// Unknown low bits (0x0F) are rejected.
proof fn scope_unknown_bits_rejected()
    ensures !scope_is_valid(0x0Fu8),
{}

/// All GLOBAL-free flag combinations (mask 0x71) are accepted.
proof fn scope_all_entry_combos_valid(v: u8)
    requires v % 16 <= 1 && v < 128,
    ensures scope_is_valid(v),
{}

pub exec fn scope_from_byte(value: u8) -> (r: Option<u8>)
    ensures r.is_some() == scope_from_byte_ok(value),
{
    if value % 16 >= 2 {
        None
    } else if value >= 128 && value != 128u8 {
        None
    } else {
        Some(value)
    }
}

fn main() {}
}

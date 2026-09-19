//! Verus pilot 2 — varint family (neo-io/src/var_int.rs), spec/proof/exec layers.
//!
//! Counterpart of the Prusti `pilot_batch3.rs` specs, cross-checking the same
//! properties with Verus/Z3 linear arithmetic:
//!   - tier ladder 1/3/5/9 with exact boundaries (incl. u64::MAX),
//!   - monotonicity of encoded length,
//!   - prefix-marker width agreement with the encoder tiers,
//!   - var-bytes total layout `encoded_len(len) + len` without usize overflow.
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus2.rs

use vstd::prelude::*;

verus! {

// --- Encoder tier ladder (mirror of `encoded_len`) ---------------------------

pub open spec fn encoded_len_spec(value: u64) -> u64 {
    if value < 253 { 1 }
    else if value <= 65535 { 3 }
    else if value <= 4294967295 { 5 }
    else { 9 }
}

pub exec fn encoded_len(value: u64) -> (r: u64)
    ensures
        r == encoded_len_spec(value),
        1 <= r <= 9,
{
    if value < 253 { 1 }
    else if value <= 65535 { 3 }
    else if value <= 4294967295 { 5 }
    else { 9 }
}

proof fn encoded_len_bounded(v: u64)
    ensures 1 <= encoded_len_spec(v) <= 9
{
}

proof fn tier_single(v: u64)
    requires v < 253,
    ensures encoded_len_spec(v) == 1,
{
}

proof fn tier_u16(v: u64)
    requires 253 <= v && v <= 65535,
    ensures encoded_len_spec(v) == 3,
{
}

proof fn tier_u32(v: u64)
    requires 65536 <= v && v <= 4294967295,
    ensures encoded_len_spec(v) == 5,
{
}

proof fn tier_u64(v: u64)
    requires v >= 4294967296,
    ensures encoded_len_spec(v) == 9,
{
}

proof fn tier_boundary_max()
    ensures encoded_len_spec(18446744073709551615u64) == 9,
{
}

// --- Monotonicity over the tier ladder ---------------------------------------

proof fn encoded_len_monotonic(a: u64, b: u64)
    requires a <= b,
    ensures encoded_len_spec(a) <= encoded_len_spec(b),
{
}

// --- Prefix marker width (mirror of `read_var_int_prefix` sizing) ------------

pub open spec fn prefix_payload_len_spec(marker: u8) -> u64 {
    match marker {
        253 => 3,
        254 => 5,
        255 => 9,
        _   => 1,
    }
}

proof fn prefix_agrees_with_encoder_u16(v: u64)
    requires 253 <= v && v <= 65535,
    ensures prefix_payload_len_spec(253u8) == encoded_len_spec(v),
{
    tier_u16(v);
}

proof fn prefix_agrees_with_encoder_u32(v: u64)
    requires 65536 <= v && v <= 4294967295,
    ensures prefix_payload_len_spec(254u8) == encoded_len_spec(v),
{
    tier_u32(v);
}

proof fn prefix_agrees_with_encoder_u64(v: u64)
    requires v >= 4294967296,
    ensures prefix_payload_len_spec(255u8) == encoded_len_spec(v),
{
    tier_u64(v);
}

// --- var-bytes layout: total = encoded_len(len) + len, no overflow -----------

pub exec fn var_bytes_total_len(len: u64) -> (r: u64)
    requires len <= 18446744073709551606u64,   // u64::MAX - 9
    ensures
        r == encoded_len_spec(len) + len,
        r >= len,
{
    let header = encoded_len(len);
    proof {
        encoded_len_bounded(len);
    }  // header <= 9, so header + len fits in u64
    header + len
}

fn main() {}
}

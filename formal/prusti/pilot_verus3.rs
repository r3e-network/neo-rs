//! Verus pilot 3 — encoding length formulas + Base58Check address validation.
//!
//! Verus counterpart of the Prusti `pilot_batch4.rs`, cross-checking the same
//! wire guarantees with Z3 (no Prusti-style restrictions: consts, match and
//! plain arithmetic are all fine here):
//!   - hex::encode length = 2n (even, monotone, overflow-guarded exec mirror),
//!   - base64 STANDARD padded length = 4*ceil(n/3) (multiple of 4, monotone),
//!   - base64 URL no-pad = padded - padding, never above padded,
//!   - decode_address_payload: length check (21) precedes version check (0x35).
//!
//! Spec fns are stated on `int` (Verus spec arithmetic is mathematical); exec
//! mirrors return usize and are tied to the spec with `as int`.
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus3.rs

use vstd::prelude::*;

verus! {

// --- Hex::encode output length -----------------------------------------------

pub open spec fn hex_len_spec(n: int) -> int {
    2 * n
}

pub exec fn hex_len(n: usize) -> (r: usize)
    requires n <= usize::MAX / 2,
    ensures r as int == hex_len_spec(n as int),
{
    2 * n
}

proof fn hex_len_even(n: int)
    requires 0 <= n,
    ensures hex_len_spec(n) % 2 == 0,
{
}

proof fn hex_len_monotonic(a: int, b: int)
    requires a <= b,
    ensures hex_len_spec(a) <= hex_len_spec(b),
{
}

proof fn hex_uint160_spot()
    ensures hex_len_spec(20) == 40,
{
}

proof fn hex_uint256_spot()
    ensures hex_len_spec(32) == 64,
{
}

// --- Base64 STANDARD (padded) output length ----------------------------------

pub open spec fn b64_padded_spec(n: int) -> int {
    4 * ((n + 2) / 3)
}

pub open spec fn b64_pad_spec(n: int) -> int {
    (3 - n % 3) % 3
}

pub open spec fn b64_nopad_spec(n: int) -> int {
    b64_padded_spec(n) - b64_pad_spec(n)
}

pub open spec fn b64_fits(n: usize) -> bool {
    4 * ((n + 2) / 3) <= usize::MAX
}

pub exec fn b64_padded(n: usize) -> (r: usize)
    requires b64_fits(n),
    ensures r as int == b64_padded_spec(n as int),
{
    4 * ((n + 2) / 3)
}

proof fn b64_padded_multiple_of_4(n: int)
    requires 0 < n,
    ensures b64_padded_spec(n) % 4 == 0,
{
}

proof fn b64_padded_monotonic(a: int, b: int)
    requires a <= b,
    ensures b64_padded_spec(a) <= b64_padded_spec(b),
{
}

proof fn b64_uint160_spot()
    ensures b64_padded_spec(20) == 28,
{
}

// --- Base64 URL-safe no-pad --------------------------------------------------

proof fn b64_pad_bounded(n: int)
    requires 0 <= n,
    ensures 0 <= b64_pad_spec(n) <= 2,
{
}

proof fn b64_nopad_nonneg(n: int)
    requires 0 <= n,
    ensures b64_nopad_spec(n) >= 0,
{
    b64_pad_bounded(n);
}

proof fn b64_nopad_le_padded(n: int)
    requires 0 <= n,
    ensures b64_nopad_spec(n) <= b64_padded_spec(n),
{
}

proof fn b64_layout_identity(n: int)
    requires 0 <= n,
    ensures b64_padded_spec(n) == b64_nopad_spec(n) + b64_pad_spec(n),
{
}

proof fn b64_nopad_spots()
    ensures
        b64_nopad_spec(0) == 0,
        b64_nopad_spec(1) == 2,
        b64_nopad_spec(2) == 3,
        b64_nopad_spec(3) == 4,
        b64_nopad_spec(4) == 6,
{
}

// --- decode_address_payload validation order ---------------------------------

pub open spec fn addr_error_spec(payload_len: int, actual: u8, expected: u8) -> u8 {
    if payload_len != 21 {
        1 // InvalidLength
    } else if actual != expected {
        2 // InvalidVersion
    } else {
        0 // Ok
    }
}

pub exec fn addr_payload_error(payload_len: usize, actual_version: u8, expected_version: u8) -> (r: u8)
    ensures r == addr_error_spec(payload_len as int, actual_version, expected_version),
{
    if payload_len != 21 {
        1
    } else if actual_version != expected_version {
        2
    } else {
        0
    }
}

proof fn addr_ok_spot()
    ensures addr_error_spec(21, 0x35u8, 0x35u8) == 0,
{
}

proof fn addr_length_precedes_version()
    ensures addr_error_spec(20, 0x41u8, 0x35u8) == 1,
{
}

proof fn addr_version_mismatch()
    ensures addr_error_spec(21, 0x41u8, 0x35u8) == 2,
{
}

fn main() {}
}

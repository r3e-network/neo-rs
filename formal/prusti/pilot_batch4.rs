//! Prusti pilot batch 4 — Phase A remainder: encoding length formulas
//! (neo-crypto/src/encoding.rs delegating to hex/base64 crates) and the
//! Base58Check address payload validation order
//! (neo-primitives/src/base58_check.rs + constants.rs).
//!
//! Wire guarantees mirrored:
//!   - hex::encode output length = 2 * input length,
//!   - base64 STANDARD (padded) output length = 4 * ceil(n/3),
//!   - base64 URL-safe no-pad output length = padded length minus padding,
//!   - decode_address_payload checks payload length (21) before version (0x35).
//!
//! Prusti 0.2.2 constraints honored (see report §4.1/§4.3):
//!   - no `<<`, no `match` in pure fns (if/else + literals only),
//!   - no `const` items inside `#[ensures]` (ICE; consts only in bodies),
//!   - every arithmetic body guarded with `requires` so overflow is impossible.

use prusti_contracts::*;

/// Mirror of `neo_primitives::constants::ADDRESS_SIZE`.
pub const ADDRESS_SIZE: usize = 20;
/// Mirror of `neo_primitives::constants::ADDRESS_VERSION` (Neo N3).
pub const ADDRESS_VERSION: u8 = 0x35;
/// Mirror of `base58_check::ADDRESS_PAYLOAD_SIZE` = version byte + UInt160.
pub const ADDRESS_PAYLOAD_SIZE: usize = 1 + ADDRESS_SIZE;

// ---- Payload size structure ----
// Prusti 0.2.2 ICEs if a #[pure] fn body references a `const` item, so the
// mirror fns use literals and the consts below are documentation only.

#[pure]
#[ensures(result == 21)]
pub fn payload_size_is_21() -> usize {
    1 + 20
}

#[pure]
#[ensures(result == 1 + 20)]
pub fn payload_size_structure() -> usize {
    1 + 20
}

#[pure]
#[ensures(result == 0x35)]
pub fn n3_address_version() -> u8 {
    0x35
}

// ---- Hex::encode output length ----------------------------------------------

/// Mirror of `hex::encode` output length guarantee. Upper bound keeps the
/// `2 * data_len` body multiplication overflow-free.
#[pure]
#[requires(data_len <= 9223372036854775807)]
#[ensures(result == 2 * data_len)]
pub fn hex_encoded_len(data_len: usize) -> usize {
    2 * data_len
}

#[pure]
#[ensures(hex_encoded_len(0) == 0)]
pub fn hex_empty() -> usize {
    hex_encoded_len(0)
}

#[pure]
#[ensures(hex_encoded_len(20) == 40)]
pub fn hex_uint160() -> usize {
    hex_encoded_len(20)
}

#[pure]
#[ensures(hex_encoded_len(32) == 64)]
pub fn hex_uint256() -> usize {
    hex_encoded_len(32)
}

#[pure]
#[requires(data_len <= 9223372036854775807)]
#[ensures(hex_encoded_len(data_len) % 2 == 0)]
pub fn hex_len_is_even(data_len: usize) -> bool {
    true
}

#[pure]
#[requires(a <= b)]
#[requires(b <= 9223372036854775807)]
#[ensures(hex_encoded_len(a) <= hex_encoded_len(b))]
pub fn hex_len_monotonic(a: usize, b: usize) -> bool {
    true
}

// ---- Base64 STANDARD (padded) output length ---------------------------------

/// Mirror of base64 STANDARD engine output length: 4 * ceil(n/3). The guard
/// keeps both `data_len + 2` and the final `4 *` free of usize overflow.
#[pure]
#[requires(data_len <= 13835058055282163707)]
#[ensures(result == 4 * ((data_len + 2) / 3))]
pub fn base64_padded_len(data_len: usize) -> usize {
    4 * ((data_len + 2) / 3)
}

#[pure]
#[ensures(base64_padded_len(0) == 0)]
pub fn b64_empty() -> usize {
    base64_padded_len(0)
}

#[pure]
#[ensures(base64_padded_len(1) == 4)]
pub fn b64_one() -> usize {
    base64_padded_len(1)
}

#[pure]
#[ensures(base64_padded_len(3) == 4)]
pub fn b64_full_group() -> usize {
    base64_padded_len(3)
}

#[pure]
#[ensures(base64_padded_len(20) == 28)]
pub fn b64_uint160() -> usize {
    base64_padded_len(20)
}

#[pure]
#[requires(0 < data_len)]
#[requires(data_len <= 13835058055282163707)]
#[ensures(base64_padded_len(data_len) % 4 == 0)]
pub fn b64_padded_multiple_of_4(data_len: usize) -> bool {
    true
}

#[pure]
#[requires(a <= b)]
#[requires(b <= 13835058055282163707)]
#[ensures(base64_padded_len(a) <= base64_padded_len(b))]
pub fn b64_padded_monotonic(a: usize, b: usize) -> bool {
    true
}

// ---- Base64 URL-safe no-pad output length -----------------------------------

/// Standard padding bytes emitted for the last (possibly partial) group.
/// Safe for every usize: `data_len % 3` is below 3.
#[pure]
#[ensures(result == (3 - data_len % 3) % 3)]
pub fn pad_len(data_len: usize) -> usize {
    (3 - data_len % 3) % 3
}

#[pure]
#[ensures(pad_len(0) == 0)]
pub fn pad_none_full() -> usize {
    pad_len(0)
}

#[pure]
#[ensures(pad_len(1) == 2)]
pub fn pad_two() -> usize {
    pad_len(1)
}

#[pure]
#[ensures(pad_len(2) == 1)]
pub fn pad_one() -> usize {
    pad_len(2)
}

/// Mirror of `Base64::url_encode_no_pad` output length.
#[pure]
#[requires(data_len <= 13835058055282163707)]
#[ensures(result == base64_padded_len(data_len) - pad_len(data_len))]
pub fn base64_nopad_len(data_len: usize) -> usize {
    base64_padded_len(data_len) - pad_len(data_len)
}

#[pure]
#[requires(data_len <= 13835058055282163707)]
#[ensures(base64_nopad_len(data_len) <= base64_padded_len(data_len))]
pub fn nopad_le_padded(data_len: usize) -> bool {
    true
}

// ---- decode_address_payload validation order --------------------------------

/// Mirror of `decode_address_payload` control flow over (len, version):
/// 0 = Ok, 1 = InvalidLength, 2 = InvalidVersion. The length check runs first.
#[pure]
pub fn address_payload_error(payload_len: usize, actual_version: u8, expected_version: u8) -> u8 {
    if payload_len != 21 {
        1
    } else if actual_version != expected_version {
        2
    } else {
        0
    }
}

#[pure]
#[ensures(address_payload_error(21, 0x35, 0x35) == 0)]
pub fn addr_ok() -> u8 {
    address_payload_error(ADDRESS_PAYLOAD_SIZE, ADDRESS_VERSION, ADDRESS_VERSION)
}

#[pure]
#[ensures(address_payload_error(20, 0x35, 0x35) == 1)]
pub fn addr_short_is_length_error() -> u8 {
    address_payload_error(20, 0x35, 0x35)
}

#[pure]
#[ensures(address_payload_error(22, 0x35, 0x35) == 1)]
pub fn addr_long_is_length_error() -> u8 {
    address_payload_error(22, 0x35, 0x35)
}

#[pure]
#[ensures(address_payload_error(21, 0x41, 0x35) == 2)]
pub fn addr_wrong_version() -> u8 {
    address_payload_error(21, 0x41, 0x35)
}

/// Precedence: a wrong length is reported even when the version is also wrong.
#[pure]
#[ensures(address_payload_error(20, 0x41, 0x35) == 1)]
pub fn addr_length_error_precedes_version() -> u8 {
    address_payload_error(20, 0x41, 0x35)
}

fn main() {}

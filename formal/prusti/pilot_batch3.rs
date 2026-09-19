//! Prusti pilot batch 3 — neo-io varint family, phase A expansion.
//! Mirrors neo-io/src/var_int.rs faithfully (if/else only: Prusti 0.2.2 pure
//! functions reject `match`; literals only: `Shl` unsupported).

use prusti_contracts::*;

/// Byte marker that widens the encoding to 2 payload bytes.
const VAR_INT_U16_MARKER: u8 = 253;
/// Byte marker that widens the encoding to 4 payload bytes.
const VAR_INT_U32_MARKER: u8 = 254;
/// Byte marker that widens the encoding to 8 payload bytes.
const VAR_INT_U64_MARKER: u8 = 255;

/// Mirror of neo-io `encoded_len`: bytes required to encode `value`.
#[pure]
#[ensures(result >= 1)]
pub fn encoded_len(value: u64) -> usize {
    if value < VAR_INT_U16_MARKER as u64 {
        1
    } else if value <= 65535 {
        3
    } else if value <= 4294967295 {
        5
    } else {
        9
    }
}

// ---- Tier spot lemmas (boundaries, incl. u64::MAX) ----

#[pure]
#[ensures(encoded_len(0) == 1)]
pub fn tier_zero() -> usize {
    encoded_len(0)
}

#[pure]
#[ensures(encoded_len(252) == 1)]
pub fn tier_max_single() -> usize {
    encoded_len(252)
}

#[pure]
#[ensures(encoded_len(253) == 3)]
pub fn tier_min_u16() -> usize {
    encoded_len(253)
}

#[pure]
#[ensures(encoded_len(65535) == 3)]
pub fn tier_max_u16() -> usize {
    encoded_len(65535)
}

#[pure]
#[ensures(encoded_len(65536) == 5)]
pub fn tier_min_u32() -> usize {
    encoded_len(65536)
}

#[pure]
#[ensures(encoded_len(4294967295) == 5)]
pub fn tier_max_u32() -> usize {
    encoded_len(4294967295)
}

#[pure]
#[ensures(encoded_len(4294967296) == 9)]
pub fn tier_min_u64() -> usize {
    encoded_len(4294967296)
}

#[pure]
#[ensures(encoded_len(18446744073709551615) == 9)]
pub fn tier_u64_max() -> usize {
    encoded_len(18446744073709551615)
}

// ---- Monotonicity: the key structural theorem of the tier ladder ----

#[pure]
#[requires(a <= b)]
#[ensures(encoded_len(a) <= encoded_len(b))]
pub fn encoded_len_monotonic(a: u64, b: u64) -> bool {
    true
}

// ---- read_var_int_prefix sizing: total bytes consumed per marker ----

/// Mirror of `read_var_int_prefix` width selection: how many bytes the
/// encoding occupies given the leading marker byte.
#[pure]
pub fn prefix_payload_len(marker: u8) -> usize {
    if marker == VAR_INT_U16_MARKER {
        3
    } else if marker == VAR_INT_U32_MARKER {
        5
    } else if marker == VAR_INT_U64_MARKER {
        9
    } else {
        1
    }
}

#[pure]
#[ensures(prefix_payload_len(252) == 1)]
pub fn prefix_single() -> usize {
    prefix_payload_len(252)
}

#[pure]
#[ensures(prefix_payload_len(253) == 3)]
pub fn prefix_u16() -> usize {
    prefix_payload_len(253)
}

#[pure]
#[ensures(prefix_payload_len(254) == 5)]
pub fn prefix_u32() -> usize {
    prefix_payload_len(254)
}

#[pure]
#[ensures(prefix_payload_len(255) == 9)]
pub fn prefix_u64() -> usize {
    prefix_payload_len(255)
}

// ---- write_var_bytes layout: total = encoded_len(len) + len ----

/// Mirror of `write_var_bytes` total output size for a payload of `len` bytes.
/// Guarded so the usize sum cannot overflow (as on the wire it cannot either).
#[pure]
#[requires(len <= 18446744073709551606)]
#[ensures(result == encoded_len(len as u64) + len)]
pub fn var_bytes_total_len(len: usize) -> usize {
    encoded_len(len as u64) + len
}

#[pure]
#[ensures(var_bytes_total_len(0) == 1)]
pub fn var_bytes_empty() -> usize {
    var_bytes_total_len(0)
}

#[pure]
#[ensures(var_bytes_total_len(253) == 256)]
pub fn var_bytes_253() -> usize {
    var_bytes_total_len(253)
}

#[pure]
#[ensures(var_bytes_total_len(65536) == 65541)]
pub fn var_bytes_65536() -> usize {
    var_bytes_total_len(65536)
}

#[pure]
#[ensures(var_bytes_total_len(4294967296) == 4294967305)]
pub fn var_bytes_u32_plus() -> usize {
    var_bytes_total_len(4294967296)
}

// ---- Consistency: prefix sizing agrees with the encoder's own tiers ----

#[pure]
#[requires(value >= 253)]
#[requires(value <= 65535)]
#[ensures(prefix_payload_len(VAR_INT_U16_MARKER) == encoded_len(value))]
pub fn prefix_matches_encoder_u16(value: u64) -> bool {
    true
}

#[pure]
#[requires(value >= 65536)]
#[requires(value <= 4294967295)]
#[ensures(prefix_payload_len(VAR_INT_U32_MARKER) == encoded_len(value))]
pub fn prefix_matches_encoder_u32(value: u64) -> bool {
    true
}

#[pure]
#[requires(value >= 4294967296)]
#[ensures(prefix_payload_len(VAR_INT_U64_MARKER) == encoded_len(value))]
pub fn prefix_matches_encoder_u64(value: u64) -> bool {
    true
}

fn main() {}

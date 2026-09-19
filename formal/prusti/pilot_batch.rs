//! Prusti pilot — batch verification of inventory Phase A pure functions.
//!
//! These mirror neo-io var_int / serializable helper semantics exactly:
//!   encoded_len(value) = 1 if value<253 else 3 if value<=65535 else 5 if
//!   value<=4294967295 else 9.  get_var_size == encoded_len.
//! Specs use Prusti contracts; run with prusti-rustc.

use prusti_contracts::*;

const VAR_INT_U16_MARKER: u64 = 253;
const U16_MAX: u64 = 0xFFFF;
const U32_MAX: u64 = 0xFFFF_FFFF;

/// Number of bytes to encode a varint (mirrors neo-io/src/var_int.rs::encoded_len).
#[pure]
#[requires(value <= u64::MAX)]
#[ensures(result == 1 || result == 3 || result == 5 || result == 9)]
pub fn encoded_len(value: u64) -> usize {
    if value < VAR_INT_U16_MARKER {
        1
    } else if value <= U16_MAX {
        3
    } else if value <= U32_MAX {
        5
    } else {
        9
    }
}

/// Small values always encode in a single byte.
#[pure]
#[requires(value < VAR_INT_U16_MARKER)]
#[ensures(encoded_len(value) == 1)]
pub fn encoded_len_small(value: u64) -> usize {
    encoded_len(value)
}

/// u16 range encodes in 3 bytes.
#[pure]
#[requires(VAR_INT_U16_MARKER <= value && value <= U16_MAX)]
#[ensures(encoded_len(value) == 3)]
pub fn encoded_len_u16(value: u64) -> usize {
    encoded_len(value)
}

/// u32 range encodes in 5 bytes.
#[pure]
#[requires(U16_MAX < value && value <= U32_MAX)]
#[ensures(encoded_len(value) == 5)]
pub fn encoded_len_u32(value: u64) -> usize {
    encoded_len(value)
}

/// Beyond u32 encodes in 9 bytes.
#[pure]
#[requires(U32_MAX < value)]
#[ensures(encoded_len(value) == 9)]
pub fn encoded_len_u64(value: u64) -> usize {
    encoded_len(value)
}

/// get_var_size mirrors encoded_len (neo-io/src/serializable/helper.rs).
#[pure]
#[requires(value <= u64::MAX)]
#[ensures(result == encoded_len(value))]
pub fn get_var_size(value: u64) -> usize {
    encoded_len(value)
}

/// Any encoded length is positive (at least one byte).
#[pure]
#[requires(value <= u64::MAX)]
#[ensures(encoded_len(value) > 0)]
pub fn encoded_len_positive(value: u64) -> usize {
    encoded_len(value)
}

fn main() {}

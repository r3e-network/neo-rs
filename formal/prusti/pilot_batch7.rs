//! Prusti pilot batch 7 — Phase C slice: block validation bounds, EC point
//! wire sizes, Bloom filter seed math.
//!
//! Faithful mirrors of:
//!   - neo-core/src/validation.rs: block size <= 2_097_152 (2 MiB),
//!     transaction count <= 512, version must be 0, primary index < validator
//!     count, timestamps >= 1_468_595_301_000 and <= now + 900_000 ms drift,
//!   - neo-crypto/src/ecc.rs: compressed/uncompressed public-key sizes
//!     (secp256r1/secp256k1: 33/65, Ed25519: 32/32),
//!   - neo-crypto/src/bloom_filter.rs: bloom_seed = hash_index *
//!     0xFBA4C795 + tweak (u32 wrapping), bit_index = hash % bit_size.
//!
//! Prusti 0.2.2 constraints honored: if/else only, literals only (no const
//! refs in pure fns), no Option in pure code, guarded arithmetic.

use prusti_contracts::*;

// ---- Block size / transaction count ----------------------------------------

/// Mirror of `validate_block_size_raw` acceptance: at most 2 MiB.
#[pure]
#[ensures(result == (block_size <= 2097152))]
pub fn block_size_ok(block_size: usize) -> bool {
    block_size <= 2097152
}

#[pure]
#[ensures(block_size_ok(0))]
pub fn block_size_empty_ok() -> bool {
    true
}

#[pure]
#[ensures(block_size_ok(2097152))]
pub fn block_size_max_ok() -> bool {
    true
}

#[pure]
#[ensures(!block_size_ok(2097153))]
pub fn block_size_over_rejected() -> bool {
    true
}

#[pure]
#[requires(a <= b)]
#[ensures(b <= 2097152 ==> a <= 2097152)]
pub fn block_size_ok_monotonic(a: usize, b: usize) -> bool {
    true
}

/// Mirror of `validate_transaction_count_raw`: at most 512 transactions.
#[pure]
#[ensures(result == (tx_count <= 512))]
pub fn tx_count_ok(tx_count: usize) -> bool {
    tx_count <= 512
}

#[pure]
#[ensures(tx_count_ok(512))]
pub fn tx_count_max_ok() -> bool {
    true
}

#[pure]
#[ensures(!tx_count_ok(513))]
pub fn tx_count_over_rejected() -> bool {
    true
}

// ---- Version / primary index -------------------------------------------------

/// Mirror of `validate_block_version`: only version 0 is supported.
#[pure]
#[ensures(result == (version == 0u32))]
pub fn block_version_ok(version: u32) -> bool {
    version == 0
}

#[pure]
#[ensures(!block_version_ok(1))]
pub fn block_version_1_rejected() -> bool {
    true
}

/// Mirror of `validate_primary_index`: primary must index a real validator.
/// `validators_count` is i32 in the real API; negative counts reject all.
#[pure]
#[requires(validators_count >= 0)]
#[ensures(result == ((primary_index as i64) < validators_count as i64))]
pub fn primary_index_ok(primary_index: u8, validators_count: i32) -> bool {
    (primary_index as i64) < validators_count as i64
}

#[pure]
#[requires(validators_count > 0)]
#[ensures(primary_index_ok(0, validators_count))]
pub fn primary_zero_always_ok(validators_count: i32) -> bool {
    true
}

#[pure]
#[ensures(!primary_index_ok(7, 7))]
pub fn primary_index_out_of_range() -> bool {
    true
}

// ---- Timestamp bounds --------------------------------------------------------

/// Mirror of the `validate_timestamp_bounds` genesis lower bound.
#[pure]
#[ensures(result == (timestamp >= 1468595301000))]
pub fn timestamp_min_ok(timestamp: u64) -> bool {
    timestamp >= 1468595301000
}

#[pure]
#[ensures(timestamp_min_ok(1468595301000))]
pub fn timestamp_genesis_ok() -> bool {
    true
}

#[pure]
#[ensures(!timestamp_min_ok(1468595300999))]
pub fn timestamp_pre_genesis_rejected() -> bool {
    true
}

/// Mirror of the future-drift upper bound: timestamp <= now + 15 minutes.
/// Guard keeps `now + 900000` free of u64 overflow.
#[pure]
#[requires(now <= 18446744073708651615)]
#[ensures(result == (timestamp <= now + 900000))]
pub fn timestamp_future_ok(timestamp: u64, now: u64) -> bool {
    timestamp <= now + 900000
}

#[pure]
#[ensures(timestamp_future_ok(1000000, 100000))]
pub fn timestamp_within_drift() -> bool {
    true
}

#[pure]
#[ensures(!timestamp_future_ok(1000001, 100000))]
pub fn timestamp_beyond_drift() -> bool {
    true
}

// ---- EC public-key wire sizes ------------------------------------------------

/// Mirror of `ECCurve::compressed_size` (0 = secp256r1, 1 = secp256k1,
/// 2 = Ed25519).
#[pure]
#[ensures(result == 33 || result == 32)]
#[ensures(curve == 2 ==> result == 32)]
pub fn compressed_size(curve: u8) -> usize {
    if curve == 2 {
        32
    } else {
        33
    }
}

/// Mirror of `ECCurve::uncompressed_size` (Ed25519 has no uncompressed form).
#[pure]
#[ensures(result == 65 || result == 32)]
#[ensures(curve == 2 ==> result == 32)]
pub fn uncompressed_size(curve: u8) -> usize {
    if curve == 2 {
        32
    } else {
        65
    }
}

#[pure]
#[ensures(compressed_size(0) == 33)]
pub fn compressed_r1() -> usize {
    compressed_size(0)
}

#[pure]
#[ensures(uncompressed_size(0) == 65)]
pub fn uncompressed_r1() -> usize {
    uncompressed_size(0)
}

#[pure]
#[ensures(compressed_size(2) == 32 && uncompressed_size(2) == 32)]
pub fn ed25519_fixed() -> bool {
    true
}

#[pure]
#[ensures(curve == 2 || compressed_size(curve) < uncompressed_size(curve))]
pub fn compressed_le_uncompressed(curve: u8) -> bool {
    true
}

// ---- Bloom filter seed / bit index -------------------------------------------
// `bloom_seed` = hash_index * 0xFBA4C795 + tweak with u32 wrapping: Prusti
// 0.2.2 cannot verify this (wrapping_add/wrapping_mul are treated as impure,
// and the u64/u128 re-encoding's multiply overflows its overflow check), so
// the seed family is verified by the Verus counterpart pilot_verus6.rs. The
// bit-index modulo property below IS provable here.

/// Mirror of `bit_index`: the hash lands inside the bit array.
/// `hash` stands in for the murmur32 output (opaque primitive).
#[pure]
#[requires(0 < bit_size)]
#[ensures(result < bit_size)]
pub fn bit_index_in_range(hash: usize, bit_size: usize) -> usize {
    hash % bit_size
}

fn main() {}

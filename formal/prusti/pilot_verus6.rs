//! Verus pilot 6 — Phase C: block validation bounds, EC wire sizes, bloom
//! filter seed math.
//!
//! Verus counterpart of the Prusti `pilot_batch7.rs`, plus the
//! `bloom_seed` wrapping-multiply family that Prusti 0.2.2 cannot verify
//! (wrapping_add/wrapping_mul are impure there and the u64/u128 re-encoding
//! trips its overflow check). Verus int semantics handles it exactly:
//! bloom_seed(hi, t) = (hi * 0xFBA4C795 + t) mod 2^32.
//!
//! Mirrors of:
//!   - neo-core/src/validation.rs bounds (2 MiB block, 512 txs, version 0,
//!     primary < validators, genesis timestamp 1468595301000, 900000 ms drift),
//!   - neo-crypto/src/ecc.rs key sizes (r1/k1: 33/65, Ed25519: 32/32),
//!   - neo-crypto/src/bloom_filter.rs seed/bit-index math.
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus6.rs

use vstd::prelude::*;

verus! {

// ---- Block validation bounds -------------------------------------------------

pub open spec fn block_size_ok(size: usize) -> bool {
    size <= 2097152
}

pub open spec fn tx_count_ok(count: usize) -> bool {
    count <= 512
}

pub open spec fn block_version_ok(version: u32) -> bool {
    version == 0
}

pub open spec fn primary_index_ok(index: u8, validators_count: i64) -> bool {
    (index as int) < validators_count
}

pub open spec fn timestamp_min_ok(ts: u64) -> bool {
    ts >= 1468595301000
}

pub open spec fn timestamp_future_ok(ts: u64, now: u64) -> bool {
    ts <= now + 900000
}

proof fn block_bounds_spots()
    ensures
        block_size_ok(2097152) && !block_size_ok(2097153),
        tx_count_ok(512) && !tx_count_ok(513),
        block_version_ok(0) && !block_version_ok(1),
        timestamp_min_ok(1468595301000) && !timestamp_min_ok(1468595300999),
{}

/// Downward closure: if a larger block fits, any smaller one does.
proof fn block_size_ok_downward(a: usize, b: usize)
    requires a <= b && block_size_ok(b),
    ensures block_size_ok(a),
{}

proof fn tx_count_ok_downward(a: usize, b: usize)
    requires a <= b && tx_count_ok(b),
    ensures tx_count_ok(a),
{}

proof fn primary_zero_always_ok(validators_count: i64)
    requires validators_count > 0,
    ensures primary_index_ok(0u8, validators_count),
{}

proof fn primary_index_out_of_range()
    ensures !primary_index_ok(7u8, 7),
{}

proof fn timestamp_drift_boundary(now: u64)
    requires now <= 18446744073708651614u64,
    ensures
        timestamp_future_ok((now + 900000) as u64, now),
        !timestamp_future_ok((now + 900001) as u64, now),
{}

// ---- EC public-key wire sizes --------------------------------------------------

pub open spec fn compressed_size(curve: u8) -> usize {
    match curve {
        2 => 32,          // Ed25519
        _ => 33,          // secp256r1 / secp256k1
    }
}

pub open spec fn uncompressed_size(curve: u8) -> usize {
    match curve {
        2 => 32,          // Ed25519 has no uncompressed form
        _ => 65,
    }
}

proof fn curve_size_spots()
    ensures
        compressed_size(0u8) == 33,
        compressed_size(1u8) == 33,
        compressed_size(2u8) == 32,
        uncompressed_size(0u8) == 65,
        uncompressed_size(2u8) == 32,
{}

proof fn compressed_below_uncompressed(curve: u8)
    requires curve != 2u8,
    ensures compressed_size(curve) < uncompressed_size(curve),
{}

// ---- Bloom filter seed / bit index ---------------------------------------------

/// Mirror of `bloom_seed`: hash_index * 0xFBA4C795 + tweak, u32 wrapping.
pub open spec fn bloom_seed_spec(hi: u32, tweak: u32) -> u32 {
    ((hi as int * 0xFBA4C795 + tweak as int) % 4294967296) as u32
}

pub exec fn bloom_seed(hi: u32, tweak: u32) -> (r: u32)
    ensures r as int == bloom_seed_spec(hi, tweak) as int,
{
    hi.wrapping_mul(0xFBA4_C795).wrapping_add(tweak)
}

proof fn bloom_seed_zero_index(tweak: u32)
    ensures bloom_seed_spec(0u32, tweak) == tweak,
{}

/// Recurrence: stepping the hash index adds the multiplier once mod 2^32.
proof fn bloom_seed_recurrence(hi: u32, tweak: u32)
    requires hi >= 1,
    ensures (bloom_seed_spec(hi, tweak) as int
             - bloom_seed_spec((hi - 1) as u32, tweak) as int + 4294967296) % 4294967296
            == 0xFBA4C795int,
{}

/// Mirror of `bit_index`: the hash lands inside the bit array.
pub open spec fn bit_index_spec(hash: usize, bit_size: usize) -> usize {
    ((hash as int) % (bit_size as int)) as usize
}

proof fn bit_index_in_range(hash: usize, bit_size: usize)
    requires 0 < bit_size,
    ensures bit_index_spec(hash, bit_size) < bit_size,
{}

fn main() {}
}

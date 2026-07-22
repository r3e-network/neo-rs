//! Blocked Bloom filters for immutable sorted index runs.
//!
//! Each published run carries one cache-line-blocked Bloom filter. A negative
//! probe is definitive, while a positive probe is verified against sorted
//! records through the sparse fence index. Filters are derived and rebuilt
//! from authenticated records whenever a run is republished.

use crate::PACK_KEY_BYTES;
use anyhow::{Context, Result, ensure};

const MIX_CONSTANT: u64 = 0x9E37_79B9_7F4A_7C15;
/// Cache-line-sized block used by production index-run format v5.
pub(crate) const BLOOM_BLOCK_BYTES: usize = 64;
/// Bits provisioned per indexed record in a blocked Bloom filter.
pub(crate) const BLOOM_BITS_PER_KEY: u64 = 12;
/// Hash probes performed inside one cache-line block.
pub(crate) const BLOOM_HASH_PROBES: u32 = 8;
const BLOOM_BLOCK_BITS: u64 = (BLOOM_BLOCK_BYTES * 8) as u64;
const BLOOM_DELTA_CONSTANT: u64 = 0xD6E8_FEB8_6659_FD93;

/// Deterministic seed-independent 64-bit hash of one 33-byte MPT node key.
pub(crate) fn key_hash(key: &[u8; PACK_KEY_BYTES]) -> u64 {
    let lane = |offset: usize| {
        u64::from_le_bytes(
            key[offset..offset + 8]
                .try_into()
                .expect("eight-byte hash lane"),
        )
    };
    let mut hash = mix64(lane(0) ^ MIX_CONSTANT);
    hash = mix64(hash ^ lane(8));
    hash = mix64(hash ^ lane(16));
    hash = mix64(hash ^ lane(24));
    mix64(hash ^ u64::from(key[32]))
}

fn seeded_hash(key_hash: u64, seed: u64) -> u64 {
    mix64(key_hash ^ seed)
}

/// Exact persisted byte length for a blocked Bloom filter over `records`.
/// The result is cache-line aligned and never zero.
pub(crate) fn blocked_bloom_bytes(records: u64) -> Result<u64> {
    ensure!(records > 0, "cannot size a Bloom filter for zero records");
    let target_bits = records
        .checked_mul(BLOOM_BITS_PER_KEY)
        .context("blocked Bloom bit count overflows")?;
    let blocks = target_bits.div_ceil(BLOOM_BLOCK_BITS).max(1);
    blocks
        .checked_mul(BLOOM_BLOCK_BYTES as u64)
        .context("blocked Bloom byte count overflows")
}

/// Incrementally built blocked Bloom filter for physical index-run format v5.
///
/// Every key touches one 64-byte block. Persisted bytes are probed directly
/// through the run mmap after publication instead of being decoded or copied.
#[derive(Debug)]
pub(crate) struct BlockedBloomFilter {
    seed: u64,
    bytes: Vec<u8>,
}

impl BlockedBloomFilter {
    /// Allocates exact bounded geometry for the indexed record count.
    pub(crate) fn with_capacity(records: u64, seed: u64) -> Result<Self> {
        let bytes = usize::try_from(blocked_bloom_bytes(records)?)
            .context("blocked Bloom byte count does not fit usize")?;
        Ok(Self {
            seed,
            bytes: vec![0u8; bytes],
        })
    }

    /// Inserts one pre-hashed key without allocating.
    pub(crate) fn insert_hash(&mut self, hash: u64) {
        let (block_start, first, delta) = bloom_probe_geometry(hash, self.seed, self.block_count());
        for probe in 0..BLOOM_HASH_PROBES {
            let bit =
                first.wrapping_add(u64::from(probe).wrapping_mul(delta)) & (BLOOM_BLOCK_BITS - 1);
            let byte = block_start + (bit as usize >> 3);
            self.bytes[byte] |= 1u8 << (bit & 7);
        }
    }

    /// Returns the exact persisted filter section.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Seed stored in the v5 run header.
    pub(crate) const fn seed(&self) -> u64 {
        self.seed
    }

    fn block_count(&self) -> usize {
        self.bytes.len() / BLOOM_BLOCK_BYTES
    }
}

/// Validates physical geometry before a mapped filter can route reads.
pub(crate) fn validate_blocked_bloom(section: &[u8], probes: u32) -> Result<()> {
    ensure!(
        !section.is_empty() && section.len().is_multiple_of(BLOOM_BLOCK_BYTES),
        "blocked Bloom section is not a non-empty sequence of cache-line blocks"
    );
    ensure!(
        probes == BLOOM_HASH_PROBES,
        "unsupported blocked Bloom probe count {probes}"
    );
    Ok(())
}

/// Probes a validated mapped filter section without decoding or copying it.
pub(crate) fn blocked_bloom_maybe_contains_hash(
    section: &[u8],
    seed: u64,
    probes: u32,
    hash: u64,
) -> bool {
    debug_assert!(validate_blocked_bloom(section, probes).is_ok());
    let blocks = section.len() / BLOOM_BLOCK_BYTES;
    let (block_start, first, delta) = bloom_probe_geometry(hash, seed, blocks);
    (0..probes).all(|probe| {
        let bit = first.wrapping_add(u64::from(probe).wrapping_mul(delta)) & (BLOOM_BLOCK_BITS - 1);
        section[block_start + (bit as usize >> 3)] & (1u8 << (bit & 7)) != 0
    })
}

fn bloom_probe_geometry(hash: u64, seed: u64, blocks: usize) -> (usize, u64, u64) {
    debug_assert!(blocks > 0);
    let seeded = seeded_hash(hash, seed);
    let block = reduce(seeded, blocks) * BLOOM_BLOCK_BYTES;
    let first = mix64(seeded.rotate_left(17) ^ MIX_CONSTANT);
    let delta = mix64(seeded ^ BLOOM_DELTA_CONSTANT) | 1;
    (block, first, delta)
}

fn reduce(hash: u64, n: usize) -> usize {
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    ((u128::from(hash) * u128::from(n)) >> 64) as usize
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(ordinal: u64) -> [u8; PACK_KEY_BYTES] {
        let mut key = [0u8; PACK_KEY_BYTES];
        let hash = mix64(ordinal ^ MIX_CONSTANT);
        key[0] = 0xf0;
        key[1..9].copy_from_slice(&hash.to_le_bytes());
        key[9..17].copy_from_slice(&mix64(hash).to_le_bytes());
        key[17..25].copy_from_slice(&ordinal.to_le_bytes());
        key
    }

    #[test]
    fn blocked_bloom_contains_every_inserted_key_with_bounded_false_positives() {
        let keys: Vec<_> = (0..20_000u64).map(test_key).collect();
        let mut filter = BlockedBloomFilter::with_capacity(keys.len() as u64, 17)
            .expect("allocate blocked Bloom");
        for key in &keys {
            filter.insert_hash(key_hash(key));
        }
        validate_blocked_bloom(filter.as_bytes(), BLOOM_HASH_PROBES)
            .expect("validate blocked Bloom");
        for key in &keys {
            assert!(
                blocked_bloom_maybe_contains_hash(
                    filter.as_bytes(),
                    filter.seed(),
                    BLOOM_HASH_PROBES,
                    key_hash(key),
                ),
                "inserted key was rejected"
            );
        }

        let probes = 200_000u64;
        let false_positives = (1_000_000..1_000_000 + probes)
            .filter(|ordinal| {
                blocked_bloom_maybe_contains_hash(
                    filter.as_bytes(),
                    filter.seed(),
                    BLOOM_HASH_PROBES,
                    key_hash(&test_key(*ordinal)),
                )
            })
            .count() as u64;
        let rate = false_positives as f64 / probes as f64;
        assert!(
            rate < 0.01,
            "blocked Bloom false-positive rate {rate} exceeds the 1% bound"
        );
    }

    #[test]
    fn blocked_bloom_geometry_is_checked_and_cache_line_aligned() {
        assert!(blocked_bloom_bytes(0).is_err());
        for records in [1, 42, 1_000, 1_000_000] {
            let bytes = blocked_bloom_bytes(records).expect("size blocked Bloom");
            assert!(bytes >= BLOOM_BLOCK_BYTES as u64);
            assert_eq!(bytes % BLOOM_BLOCK_BYTES as u64, 0);
        }
        assert!(validate_blocked_bloom(&[], BLOOM_HASH_PROBES).is_err());
        assert!(validate_blocked_bloom(&[0u8; 63], BLOOM_HASH_PROBES).is_err());
        assert!(validate_blocked_bloom(&[0u8; 64], BLOOM_HASH_PROBES + 1).is_err());
    }
}

//! Xor16 membership filters for immutable sorted index runs.
//!
//! Each published run carries a xor filter (Graf & Lemire, "Xor Filters:
//! Faster and Smaller Than Bloom and Cuckoo Filters") built over the run's
//! distinct keys. A negative probe is definitive, so point and batch reads can
//! skip a run without touching its records; a positive probe is verified
//! against the sorted records through the sparse fence index. The filter is a
//! derived structure: it is rebuilt from the run records whenever the run is
//! republished.
//!
//! Key hashing is split in two so one key hash serves every run: `key_hash`
//! mixes the 33-byte key once per lookup, and each run reseeds it with a
//! single `mix64` per probe.

use crate::PACK_KEY_BYTES;
use anyhow::{Context, Result, ensure};

/// Fingerprint width in bits for the run membership filter.
pub(crate) const FILTER_FINGERPRINT_BITS: u32 = 16;
/// Bound on deterministic reseeding attempts before a build is rejected.
const MAX_BUILD_ATTEMPTS: u32 = 64;
const MIX_CONSTANT: u64 = 0x9E37_79B9_7F4A_7C15;

/// Cache-line-sized block used by physical index-run format v4.
pub(crate) const BLOOM_BLOCK_BYTES: usize = 64;
/// Bits provisioned per distinct key in a v4 blocked Bloom filter.
pub(crate) const BLOOM_BITS_PER_KEY: u64 = 12;
/// Hash probes performed inside one cache-line block.
pub(crate) const BLOOM_HASH_PROBES: u32 = 8;
const BLOOM_BLOCK_BITS: u64 = (BLOOM_BLOCK_BYTES * 8) as u64;
const BLOOM_DELTA_CONSTANT: u64 = 0xD6E8_FEB8_6659_FD93;

/// Fingerprint count for a filter over `distinct` unique keys.
pub(crate) fn filter_capacity(distinct: usize) -> usize {
    let desired = 32usize.saturating_add(distinct.saturating_mul(123).saturating_add(99) / 100);
    desired / 3 * 3
}

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

/// Reseeds a key hash for one run's filter.
fn seeded_hash(key_hash: u64, seed: u64) -> u64 {
    mix64(key_hash ^ seed)
}

/// In-memory xor16 filter for one immutable run.
#[derive(Debug, Clone)]
pub(crate) struct XorFilter {
    seed: u64,
    block_length: usize,
    fingerprints: Vec<u16>,
}

impl XorFilter {
    /// Builds a filter over `keys`, which must be strictly increasing.
    pub(crate) fn build(keys: &[[u8; PACK_KEY_BYTES]], seed: u64) -> Result<Self> {
        ensure!(!keys.is_empty(), "cannot build a filter over zero keys");
        ensure!(
            keys.windows(2).all(|pair| pair[0] < pair[1]),
            "filter keys must be strictly increasing"
        );
        let capacity = filter_capacity(keys.len());
        let block_length = capacity / 3;
        let key_hashes: Vec<u64> = keys.iter().map(key_hash).collect();
        for attempt in 0..MAX_BUILD_ATTEMPTS {
            let seed = seed.wrapping_add(u64::from(attempt));
            let Some(filter) = Self::try_build(&key_hashes, seed, block_length) else {
                continue;
            };
            filter.self_check(&key_hashes)?;
            return Ok(filter);
        }
        anyhow::bail!("xor filter construction exhausted {MAX_BUILD_ATTEMPTS} seeds")
    }

    /// Decodes a persisted filter section (little-endian u16 fingerprints).
    pub(crate) fn decode(seed: u64, section: &[u8]) -> Result<Self> {
        ensure!(
            section.len().is_multiple_of(2),
            "xor filter section has an odd byte length"
        );
        let count = section.len() / 2;
        ensure!(
            count >= 33 && count.is_multiple_of(3),
            "xor filter fingerprint count {count} is not a valid capacity"
        );
        let mut fingerprints = Vec::with_capacity(count);
        for chunk in section.chunks_exact(2) {
            fingerprints.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        Ok(Self {
            seed,
            block_length: count / 3,
            fingerprints,
        })
    }

    /// Hash-reusing probe for newest-first scans across many runs.
    pub(crate) fn maybe_contains_hash(&self, key_hash: u64) -> bool {
        let hash = seeded_hash(key_hash, self.seed);
        let [r0, r1, r2] = positions(hash, self.block_length);
        fingerprint(hash) == self.fingerprints[r0] ^ self.fingerprints[r1] ^ self.fingerprints[r2]
    }

    /// Seed used by every probe of this filter.
    pub(crate) const fn seed(&self) -> u64 {
        self.seed
    }

    /// Number of persisted u16 fingerprints.
    pub(crate) fn fingerprint_count(&self) -> usize {
        self.fingerprints.len()
    }

    /// Little-endian u16 fingerprint section bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.fingerprints.len() * 2);
        for fingerprint in &self.fingerprints {
            bytes.extend_from_slice(&fingerprint.to_le_bytes());
        }
        bytes
    }

    /// Estimated resident bytes of the decoded filter.
    pub(crate) fn memory_bytes(&self) -> u64 {
        u64::try_from(self.fingerprints.len() * 2).unwrap_or(u64::MAX)
    }

    fn try_build(key_hashes: &[u64], seed: u64, block_length: usize) -> Option<Self> {
        let capacity = block_length * 3;
        let mut counts = vec![0u8; capacity];
        let mut xors = vec![0u64; capacity];
        for &key_hash in key_hashes {
            let hash = seeded_hash(key_hash, seed);
            for position in positions(hash, block_length) {
                counts[position] += 1;
                xors[position] ^= hash;
            }
        }
        let mut queue: Vec<usize> = (0..capacity).filter(|index| counts[*index] == 1).collect();
        let mut peeled: Vec<(usize, u64)> = Vec::with_capacity(key_hashes.len());
        let mut head = 0usize;
        while head < queue.len() {
            let position = queue[head];
            head += 1;
            if counts[position] != 1 {
                continue;
            }
            let hash = xors[position];
            peeled.push((position, hash));
            for other in positions(hash, block_length) {
                if other == position {
                    continue;
                }
                xors[other] ^= hash;
                counts[other] -= 1;
                if counts[other] == 1 {
                    queue.push(other);
                }
            }
            counts[position] = 0;
        }
        if peeled.len() != key_hashes.len() {
            return None;
        }
        let mut fingerprints = vec![0u16; capacity];
        for &(position, hash) in peeled.iter().rev() {
            let [r0, r1, r2] = positions(hash, block_length);
            fingerprints[position] =
                fingerprint(hash) ^ fingerprints[r0] ^ fingerprints[r1] ^ fingerprints[r2];
        }
        Some(Self {
            seed,
            block_length,
            fingerprints,
        })
    }

    fn self_check(&self, key_hashes: &[u64]) -> Result<()> {
        for &key_hash in key_hashes {
            ensure!(
                self.maybe_contains_hash(key_hash),
                "xor filter rejected an inserted key"
            );
        }
        Ok(())
    }
}

/// Exact persisted byte length for a blocked Bloom filter over `distinct`
/// keys. The result is cache-line aligned and never zero.
pub(crate) fn blocked_bloom_bytes(distinct: u64) -> Result<u64> {
    ensure!(distinct > 0, "cannot size a Bloom filter for zero keys");
    let target_bits = distinct
        .checked_mul(BLOOM_BITS_PER_KEY)
        .context("blocked Bloom bit count overflows")?;
    let blocks = target_bits.div_ceil(BLOOM_BLOCK_BITS).max(1);
    blocks
        .checked_mul(BLOOM_BLOCK_BYTES as u64)
        .context("blocked Bloom byte count overflows")
}

/// Incrementally built blocked Bloom filter for physical index-run format v4.
///
/// Every key touches one 64-byte block. The persisted bytes can therefore be
/// probed directly through the run mmap after publication instead of keeping
/// the complete filter in a second resident allocation.
#[derive(Debug)]
pub(crate) struct BlockedBloomFilter {
    seed: u64,
    bytes: Vec<u8>,
}

impl BlockedBloomFilter {
    /// Allocates the exact bounded filter geometry for the known distinct-key
    /// count produced by compaction pass one.
    pub(crate) fn with_capacity(distinct: u64, seed: u64) -> Result<Self> {
        let bytes = usize::try_from(blocked_bloom_bytes(distinct)?)
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

    /// Seed stored in the v4 run header.
    pub(crate) const fn seed(&self) -> u64 {
        self.seed
    }

    fn block_count(&self) -> usize {
        self.bytes.len() / BLOOM_BLOCK_BYTES
    }
}

/// Validates the physical geometry before a mapped v4 filter can route reads.
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

/// Probes a validated mapped v4 filter section without decoding or copying it.
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

/// Three fingerprint positions for one hash (Graf & Lemire rotation scheme).
///
/// Positions must be derived with the multiply-shift `reduce` on top bits,
/// never with modulo: for an odd block length `b`, `rotl(h, k) % b` is
/// affinely related to `h % b` through the constant `2^k mod b`, so
/// modulo-derived positions correlate and the build-time peeling stalls in a
/// permanent 2-core (reproduced: 100/100 construction failures at n=256,
/// block=115). Top-bit windowing decorrelates the three rotations.
fn positions(hash: u64, block_length: usize) -> [usize; 3] {
    [
        reduce(hash, block_length),
        block_length + reduce(hash.rotate_left(21), block_length),
        2 * block_length + reduce(hash.rotate_left(42), block_length),
    ]
}

/// Maps a hash to `[0, n)` via the top bits of the 128-bit product.
fn reduce(hash: u64, n: usize) -> usize {
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    ((u128::from(hash) * u128::from(n)) >> 64) as usize
}

fn fingerprint(hash: u64) -> u16 {
    (hash >> 48) as u16
}

/// splitmix64 finalizer.
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

    fn contains(filter: &XorFilter, key: &[u8; PACK_KEY_BYTES]) -> bool {
        filter.maybe_contains_hash(key_hash(key))
    }

    #[test]
    fn filter_contains_every_inserted_key_with_rare_false_positives() {
        let mut keys: Vec<_> = (0..20_000u64).map(test_key).collect();
        keys.sort();
        let filter = XorFilter::build(&keys, 7).expect("build filter");
        for key in &keys {
            assert!(contains(&filter, key), "inserted key rejected");
        }
        let mut false_positives = 0u64;
        let probes = 200_000u64;
        for ordinal in 1_000_000..1_000_000 + probes {
            if contains(&filter, &test_key(ordinal)) {
                false_positives += 1;
            }
        }
        let rate = false_positives as f64 / probes as f64;
        assert!(
            rate < 0.001,
            "xor16 false-positive rate {rate} exceeds the 0.1% bound"
        );
    }

    #[test]
    fn filter_encoding_roundtrips() {
        let mut keys: Vec<_> = (0..1_024u64).map(test_key).collect();
        keys.sort();
        let filter = XorFilter::build(&keys, 11).expect("build filter");
        let decoded = XorFilter::decode(filter.seed(), &filter.encode()).expect("decode filter");
        assert_eq!(decoded.fingerprint_count(), filter.fingerprint_count());
        for ordinal in 0..2_048u64 {
            let key = test_key(ordinal);
            assert_eq!(
                contains(&filter, &key),
                contains(&decoded, &key),
                "decoded filter diverged at ordinal {ordinal}"
            );
        }
        assert!(XorFilter::decode(0, &[0u8; 3]).is_err());
        assert!(XorFilter::decode(0, &[0u8; 62]).is_err());
    }

    #[test]
    fn filter_handles_small_and_duplicate_rejecting_inputs() {
        for count in 1..8u64 {
            let mut keys: Vec<_> = (0..count).map(test_key).collect();
            keys.sort();
            let filter = XorFilter::build(&keys, 3).expect("build small filter");
            for key in &keys {
                assert!(contains(&filter, key));
            }
        }
        let mut duplicated: Vec<_> = (0..4u64).map(test_key).collect();
        duplicated.push(test_key(0));
        duplicated.sort();
        assert!(XorFilter::build(&duplicated, 3).is_err());
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
        for distinct in [1, 42, 1_000, 1_000_000] {
            let bytes = blocked_bloom_bytes(distinct).expect("size blocked Bloom");
            assert!(bytes >= BLOOM_BLOCK_BYTES as u64);
            assert_eq!(bytes % BLOOM_BLOCK_BYTES as u64, 0);
        }
        assert!(validate_blocked_bloom(&[], BLOOM_HASH_PROBES).is_err());
        assert!(validate_blocked_bloom(&[0u8; 63], BLOOM_HASH_PROBES).is_err());
        assert!(validate_blocked_bloom(&[0u8; 64], BLOOM_HASH_PROBES + 1).is_err());
    }
}

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
use anyhow::{Result, ensure};

/// Fingerprint width in bits for the run membership filter.
pub(crate) const FILTER_FINGERPRINT_BITS: u32 = 16;
/// Bound on deterministic reseeding attempts before a build is rejected.
const MAX_BUILD_ATTEMPTS: u32 = 64;
const MIX_CONSTANT: u64 = 0x9E37_79B9_7F4A_7C15;

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
}

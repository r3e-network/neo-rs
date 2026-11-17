//! Helper extensions for TokensTracker.
//!
//! Utility functions for serialization size calculation and database queries.

use base64::Engine;
use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{MemoryReader, Serializable};
use neo_storage::persistence::{SeekDirection, Store};
use num_bigint::BigInt;

/// Records the scanner may read per accepted result.
///
/// Callers filter raw tracker rows after they are read (unknown contracts,
/// missing metadata), so a small surplus of reads is required to fill a page.
/// Without a cap a filter that discards most rows would still walk the whole
/// keyspace.
pub const SCAN_LIMIT_MULTIPLIER: usize = 10;

/// Upper bound on rows read while collecting at most `max_results` matches.
#[must_use]
pub fn scan_limit(max_results: usize) -> usize {
    max_results
        .saturating_mul(SCAN_LIMIT_MULTIPLIER)
        .max(max_results)
}

/// Converts a byte slice to a Base64 string (empty slice -> empty string).
pub fn to_base64(data: &[u8]) -> String {
    if data.is_empty() {
        String::new()
    } else {
        base64::engine::general_purpose::STANDARD.encode(data)
    }
}

/// Returns the Neo var-size contribution of a BigInt.
pub fn bigint_var_size(value: &BigInt) -> usize {
    let bytes = value.to_signed_bytes_le();
    get_var_size_bytes(&bytes)
}

/// Finds entries whose keys start with the given prefix, stopping as soon as
/// `max_results` entries are collected or [`scan_limit`] records have been read.
pub fn find_prefix<TKey, TValue>(
    db: &dyn Store,
    prefix: &[u8],
    max_results: usize,
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    find_prefix_with_limits(db, prefix, max_results, scan_limit(max_results))
}

/// `find_prefix` with an explicit scan budget.
pub fn find_prefix_with_limits<TKey, TValue>(
    db: &dyn Store,
    prefix: &[u8],
    max_results: usize,
    max_scan_count: usize,
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    if max_results == 0 || max_scan_count == 0 {
        return Ok(Vec::new());
    }

    let prefix_vec = prefix.to_vec();
    let mut results = Vec::with_capacity(max_results.min(64));
    let mut scanned = 0usize;

    let snapshot = db.snapshot();
    for (key_bytes, value_bytes) in snapshot.find(Some(&prefix_vec), SeekDirection::Forward) {
        if !key_bytes.starts_with(prefix)
            || results.len() >= max_results
            || scanned >= max_scan_count
        {
            break;
        }
        scanned += 1;

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value = TValue::deserialize(&mut value_reader).map_err(|e| e.to_string())?;

        results.push((key, value));
    }

    Ok(results)
}

/// Finds entries in the inclusive range [start_key, end_key], stopping as soon
/// as `max_results` entries are collected or [`scan_limit`] records have been
/// read (including records discarded by the range bounds).
pub fn find_range<TKey, TValue>(
    db: &dyn Store,
    start_key: &[u8],
    end_key: &[u8],
    max_results: usize,
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    find_range_with_limits(db, start_key, end_key, max_results, scan_limit(max_results))
}

/// `find_range` with an explicit scan budget.
pub fn find_range_with_limits<TKey, TValue>(
    db: &dyn Store,
    start_key: &[u8],
    end_key: &[u8],
    max_results: usize,
    max_scan_count: usize,
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    if max_results == 0 || max_scan_count == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::with_capacity(max_results.min(64));
    let mut scanned = 0usize;

    // `find` only exposes prefix semantics: it returns keys that start with
    // the given prefix. Scanning from `start_key` itself (a full lower-bound
    // key that embeds a timestamp) would return nothing unless some record
    // starts with those exact bytes. The former store implementation leaked a
    // superset for forward seeks (every key >= start_key), which this helper
    // silently relied on to emulate a range scan; with correct prefix
    // semantics we derive the longest common prefix of both bounds instead,
    // so the scan covers every record that can fall inside the range, then
    // filter by the bounds explicitly.
    let common_len = start_key
        .iter()
        .zip(end_key.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let scan_prefix = start_key[..common_len].to_vec();

    let snapshot = db.snapshot();
    for (key_bytes, value_bytes) in snapshot.find(Some(&scan_prefix), SeekDirection::Forward) {
        if key_bytes.as_slice() > end_key
            || results.len() >= max_results
            || scanned >= max_scan_count
        {
            break;
        }
        scanned += 1;
        if key_bytes.as_slice() < start_key {
            continue;
        }

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value = TValue::deserialize(&mut value_reader).map_err(|e| e.to_string())?;

        results.push((key, value));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, UInt256};
    use neo_storage::persistence::providers::MemoryStore;
    use neo_storage::persistence::write_store::WriteStore;

    // `Serializable` is only implemented for neo primitives, so the tests key
    // records by a 32-byte hash with the ordering index in its leading bytes.
    fn key(prefix: u8, index: u32) -> Vec<u8> {
        let mut bytes = vec![prefix];
        let mut hash = [0u8; 32];
        hash[..4].copy_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&hash);
        bytes
    }

    fn seeded_store(count: u32) -> MemoryStore {
        let mut store = MemoryStore::new();
        for index in 0..count {
            store
                .put(key(0x01, index), vec![index as u8; 20])
                .expect("seed record");
        }
        store
    }

    #[test]
    fn find_prefix_stops_once_max_results_is_reached() {
        let store = seeded_store(50);
        let found = find_prefix::<UInt256, UInt160>(&store as &dyn Store, &[0x01], 5)
            .expect("prefix scan");
        assert_eq!(found.len(), 5);
    }

    #[test]
    fn find_prefix_honours_scan_limit() {
        let store = seeded_store(50);
        // The page is not filled before the scan cap is hit, so the cap (not
        // max_results) must end the scan.
        let found = find_prefix_with_limits::<UInt256, UInt160>(&store as &dyn Store, &[0x01], 50, 3)
            .expect("prefix scan");
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn find_range_stops_once_max_results_is_reached() {
        let store = seeded_store(50);
        let found = find_range::<UInt256, UInt160>(
            &store as &dyn Store,
            &key(0x01, 0),
            &key(0x01, 49),
            4,
        )
        .expect("range scan");
        assert_eq!(found.len(), 4);
    }
}

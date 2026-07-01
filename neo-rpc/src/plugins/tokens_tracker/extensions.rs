//! Helper extensions for TokensTracker.
//!
//! Utility functions for serialization size calculation and database queries.

use base64::Engine;
use neo_error::{CoreError, CoreResult};
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{MemoryReader, Serializable};
use neo_storage::persistence::{SeekDirection, Store};
use num_bigint::BigInt;

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
    SerializeHelper::get_var_size_bytes(&bytes)
}

/// Finds entries whose keys start with the given prefix.
pub fn find_prefix<S, TKey, TValue>(db: &S, prefix: &[u8]) -> CoreResult<Vec<(TKey, TValue)>>
where
    S: Store + ?Sized,
    TKey: Serializable,
    TValue: Serializable,
{
    let prefix_vec = prefix.to_vec();
    let mut results = Vec::new();

    let snapshot = db.snapshot();
    for (key_bytes, value_bytes) in snapshot.find(Some(&prefix_vec), SeekDirection::Forward) {
        if !key_bytes.starts_with(prefix) {
            break;
        }

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key =
            TKey::deserialize(&mut key_reader).map_err(|e| CoreError::other(e.to_string()))?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value =
            TValue::deserialize(&mut value_reader).map_err(|e| CoreError::other(e.to_string()))?;

        results.push((key, value));
    }

    Ok(results)
}

/// Finds entries in the inclusive range [start_key, end_key].
pub fn find_range<S, TKey, TValue>(
    db: &S,
    start_key: &[u8],
    end_key: &[u8],
) -> CoreResult<Vec<(TKey, TValue)>>
where
    S: Store + ?Sized,
    TKey: Serializable,
    TValue: Serializable,
{
    if start_key > end_key {
        return Ok(Vec::new());
    }

    let shared_prefix_len = start_key
        .iter()
        .zip(end_key.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let shared_prefix = start_key[..shared_prefix_len].to_vec();
    let mut results = Vec::new();

    let snapshot = db.snapshot();
    for (key_bytes, value_bytes) in snapshot.find(
        (!shared_prefix.is_empty()).then_some(&shared_prefix),
        SeekDirection::Forward,
    ) {
        if key_bytes.as_slice() < start_key {
            continue;
        }
        if key_bytes.as_slice() > end_key {
            break;
        }

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key =
            TKey::deserialize(&mut key_reader).map_err(|e| CoreError::other(e.to_string()))?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value =
            TValue::deserialize(&mut value_reader).map_err(|e| CoreError::other(e.to_string()))?;

        results.push((key, value));
    }

    Ok(results)
}

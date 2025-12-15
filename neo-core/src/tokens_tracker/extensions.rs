//! Helper extensions for TokensTracker.
//!
//! Utility functions for serialization size calculation and database queries.

use base64::Engine;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{MemoryReader, Serializable};
use crate::persistence::{IStore, SeekDirection};
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
    get_var_size(bytes.len() as u64) + bytes.len()
}

/// Returns the Neo var-size contribution of a byte slice.
pub fn bytes_var_size(len: usize) -> usize {
    get_var_size(len as u64) + len
}

/// Finds entries whose keys start with the given prefix.
pub fn find_prefix<TKey, TValue>(
    db: &dyn IStore,
    prefix: &[u8],
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    let prefix_vec = prefix.to_vec();
    let mut results = Vec::new();

    let snapshot = db.get_snapshot();
    for (key_bytes, value_bytes) in snapshot.find(Some(&prefix_vec), SeekDirection::Forward) {
        if !key_bytes.starts_with(prefix) {
            break;
        }

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value = TValue::deserialize(&mut value_reader).map_err(|e| e.to_string())?;

        results.push((key, value));
    }

    Ok(results)
}

/// Finds entries in the inclusive range [start_key, end_key].
pub fn find_range<TKey, TValue>(
    db: &dyn IStore,
    start_key: &[u8],
    end_key: &[u8],
) -> Result<Vec<(TKey, TValue)>, String>
where
    TKey: Serializable,
    TValue: Serializable,
{
    let start_vec = start_key.to_vec();
    let mut results = Vec::new();

    let snapshot = db.get_snapshot();
    for (key_bytes, value_bytes) in snapshot.find(Some(&start_vec), SeekDirection::Forward) {
        if key_bytes.as_slice() > end_key {
            break;
        }

        let mut key_reader = MemoryReader::new(&key_bytes[1..]);
        let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

        let mut value_reader = MemoryReader::new(&value_bytes);
        let value = TValue::deserialize(&mut value_reader).map_err(|e| e.to_string())?;

        results.push((key, value));
    }

    Ok(results)
}

//! Strict diagnostics for canonical Neo state-root persistence records.

use neo_primitives::UInt256;
use neo_storage::persistence::StoreCache;
use neo_system::state_service::commit_handlers::read_state_root_record;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRootCheck {
    pub index: u32,
    pub root_hash: UInt256,
    pub version: u8,
    pub witness_present: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateCheckError {
    #[error("state-root record is missing at height {0}")]
    Missing(u32),
    #[error("state-root record at height {index} contains the invalid zero root")]
    ZeroRoot { index: u32 },
    #[error("state-root mismatch at height {index}: expected {expected}, actual {actual}")]
    Mismatch {
        index: u32,
        expected: UInt256,
        actual: UInt256,
    },
    #[error("failed to decode state-root record: {0}")]
    Malformed(String),
}

/// Checks presence, canonical decoding and an optional expected root.
///
/// Record keys and value decoding remain owned by `neo-system`; this crate does
/// not duplicate the persistence format.
pub fn check_state_root(
    store_cache: &StoreCache,
    index: u32,
    expected_root: Option<UInt256>,
) -> Result<StateRootCheck, StateCheckError> {
    let record = read_state_root_record(store_cache, index)
        .map_err(StateCheckError::Malformed)?
        .ok_or(StateCheckError::Missing(index))?;
    if record.root_hash == UInt256::zero() {
        return Err(StateCheckError::ZeroRoot { index });
    }
    if let Some(expected) = expected_root {
        if expected != record.root_hash {
            return Err(StateCheckError::Mismatch {
                index,
                expected,
                actual: record.root_hash,
            });
        }
    }
    Ok(StateRootCheck {
        index,
        root_hash: record.root_hash,
        version: record.version,
        witness_present: record.witness.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::DataCache;
    use neo_storage::{StorageItem, StorageKey};
    use std::sync::Arc;

    fn cache_with_record(index: u32, value: Vec<u8>) -> StoreCache {
        let store = Arc::new(MemoryStore::new());
        let cache = StoreCache::new(store);
        cache
            .data_cache()
            .try_add(
                StorageKey::create_with_uint32(i32::MIN, 0xf0, index),
                StorageItem::from_bytes(value),
            )
            .expect("insert state-root fixture");
        cache
    }

    fn unsigned_record(root: UInt256) -> Vec<u8> {
        let mut value = root.to_bytes().to_vec();
        value.extend_from_slice(&[0, 0]);
        value
    }

    #[test]
    fn reads_canonical_record_and_checks_expected_root() {
        let root = UInt256::from([7; 32]);
        let cache = cache_with_record(9, unsigned_record(root));
        let result = check_state_root(&cache, 9, Some(root)).expect("valid state root");
        assert_eq!(result.index, 9);
        assert_eq!(result.root_hash, root);
        assert!(!result.witness_present);
    }

    #[test]
    fn missing_malformed_zero_and_mismatched_roots_fail_closed() {
        let root = UInt256::from([7; 32]);
        let cache = cache_with_record(9, unsigned_record(root));
        assert_eq!(
            check_state_root(&cache, 10, None),
            Err(StateCheckError::Missing(10))
        );
        assert!(matches!(
            check_state_root(&cache, 9, Some(UInt256::from([8; 32]))),
            Err(StateCheckError::Mismatch { index: 9, .. })
        ));

        let malformed = cache_with_record(2, vec![0; 33]);
        assert!(matches!(
            check_state_root(&malformed, 2, None),
            Err(StateCheckError::Malformed(_))
        ));
        let zero = cache_with_record(3, unsigned_record(UInt256::zero()));
        assert_eq!(
            check_state_root(&zero, 3, None),
            Err(StateCheckError::ZeroRoot { index: 3 })
        );
    }
}

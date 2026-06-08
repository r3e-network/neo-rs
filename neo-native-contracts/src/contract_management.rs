//! ContractManagement native contract.
//!
//! Concrete implementation of the read-side surface of the
//! ContractManagement native contract. The full deploy / update /
//! destroy mutating surface lives in the `neo-blockchain` reth-style
//! service (which writes the storage entries this module reads), but
//! the read surface (look up a deployed contract by hash) is consumed
//! by oracle service, RPC, the application engine, and the tokens
//! tracker, so it lives here so all those crates can share it without
//! depending on `neo-blockchain`.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::ContractState;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::StorageKey;
use std::sync::LazyLock;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id → hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
const PREFIX_CONTRACT_HASH: u8 = 12;

/// Lazily-initialised script-hash handle for the ContractManagement contract.
pub static CONTRACT_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *CONTRACT_MANAGEMENT_HASH);

/// Static accessor for the ContractManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContractManagement;

impl ContractManagement {
    /// Stable native contract id (matches C# `ContractManagement.Id`).
    pub const ID: i32 = -1;

    /// Constructs a new `ContractManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the ContractManagement contract.
    pub fn hash(&self) -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH_REF
    }

    /// Returns the script hash of the ContractManagement contract (static).
    pub fn script_hash() -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH_REF
    }

    /// Looks up a deployed contract by its script hash.
    ///
    /// Reads the per-contract record (`prefix 8` + `hash.to_bytes()`)
    /// previously written by `ContractManagement.Deploy` /
    /// `ContractManagement.Update` in the blockchain service.
    pub fn get_contract_from_snapshot(
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let key = StorageKey::new(Self::ID, contract_storage_key(hash));
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };

        let bytes = item.value_bytes().into_owned();
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut reader = MemoryReader::new(&bytes);
        let state = ContractState::deserialize(&mut reader).map_err(|e| {
            CoreError::deserialization(format!("Failed to deserialize contract state: {e}"))
        })?;
        Ok(Some(state))
    }

    /// Looks up a deployed contract by its contract id.
    ///
    /// Reads the contract-id → hash index (`prefix 12` + `id_be_bytes`)
    /// then dereferences the resulting hash via `get_contract_from_snapshot`.
    pub fn get_contract_by_id_from_snapshot(
        snapshot: &DataCache,
        id: i32,
    ) -> CoreResult<Option<ContractState>> {
        let id_key = StorageKey::new(Self::ID, contract_id_storage_key(id));
        let hash_bytes = match snapshot.get(&id_key) {
            Some(item) => item.value_bytes().into_owned(),
            None => {
                // Fall back to the legacy LE encoding for older snapshots.
                let legacy = StorageKey::new(Self::ID, contract_id_storage_key_legacy(id));
                match snapshot.get(&legacy) {
                    Some(item) => item.value_bytes().into_owned(),
                    None => return Ok(None),
                }
            }
        };

        if hash_bytes.is_empty() {
            return Ok(None);
        }

        let hash = UInt160::from_bytes(&hash_bytes).map_err(|e| {
            CoreError::invalid_data(format!("Invalid contract hash bytes: {e}"))
        })?;
        Self::get_contract_from_snapshot(snapshot, &hash)
    }

    /// Checks whether a contract is deployed in the given snapshot.
    pub fn is_contract(snapshot: &DataCache, hash: &UInt160) -> bool {
        let key = StorageKey::new(Self::ID, contract_storage_key(hash));
        snapshot.get(&key).is_some()
    }
}

#[inline]
fn contract_storage_key(hash: &UInt160) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 20);
    key.push(PREFIX_CONTRACT);
    key.extend_from_slice(&hash.to_bytes());
    key
}

#[inline]
fn contract_id_storage_key(id: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 4);
    key.push(PREFIX_CONTRACT_HASH);
    key.extend_from_slice(&id.to_be_bytes());
    key
}

#[inline]
fn contract_id_storage_key_legacy(id: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 4);
    key.push(PREFIX_CONTRACT_HASH);
    key.extend_from_slice(&id.to_le_bytes());
    key
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use neo_data_cache::DataCache;
    use neo_io::BinaryWriter;
    use neo_storage::StorageItem;
    use std::sync::Arc;

    fn fresh_cache() -> Arc<DataCache> {
        Arc::new(DataCache::new_with_config(
            false,
            None,
            None,
            Default::default(),
        ))
    }

    fn sample_hash(byte: u8) -> UInt160 {
        UInt160::from_bytes(&[byte; 20]).unwrap()
    }

    #[test]
    fn test_contract_management_constants() {
        assert_eq!(ContractManagement::ID, -1);
    }

    #[test]
    fn test_contract_management_hash() {
        let expected = *CONTRACT_MANAGEMENT_HASH;
        assert_eq!(ContractManagement::script_hash(), expected);
        assert_eq!(ContractManagement::new().hash(), expected);
    }

    #[test]
    fn test_is_contract_false_when_missing() {
        let cache = fresh_cache();
        assert!(!ContractManagement::is_contract(&cache, &sample_hash(1)));
    }

    #[test]
    fn test_get_contract_returns_none_when_missing() {
        let cache = fresh_cache();
        let res = ContractManagement::get_contract_from_snapshot(&cache, &sample_hash(1)).unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn test_get_contract_by_id_returns_none_when_missing() {
        let cache = fresh_cache();
        let res = ContractManagement::get_contract_by_id_from_snapshot(&cache, 42).unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn test_contract_storage_key_format() {
        let key = StorageKey::new(ContractManagement::ID, {
            let mut k = vec![PREFIX_CONTRACT];
            k.extend_from_slice(&sample_hash(1).to_bytes());
            k
        });
        assert_eq!(key.id(), ContractManagement::ID);
        assert_eq!(key.key()[0], PREFIX_CONTRACT);
        assert_eq!(key.key().len(), 21);
    }

    #[test]
    fn test_contract_id_storage_key_be() {
        let key = StorageKey::new(ContractManagement::ID, {
            let mut k = vec![PREFIX_CONTRACT_HASH];
            k.extend_from_slice(&42i32.to_be_bytes());
            k
        });
        assert_eq!(key.id(), ContractManagement::ID);
        assert_eq!(key.key()[0], PREFIX_CONTRACT_HASH);
        // 1 prefix + 4 id bytes (big-endian)
        assert_eq!(key.key().len(), 5);
        assert_eq!(&key.key()[1..], &42i32.to_be_bytes());
    }

    #[test]
    fn test_write_then_read_contract() {
        let cache = fresh_cache();
        let hash = sample_hash(7);

        // Synthesise a serialised ContractState. The real one is
        // constructed by the blockchain service; we just need any
        // payload that round-trips for the storage check.
        let payload = {
            let mut w = BinaryWriter::new();
            w.write_u16(0) // version
             .unwrap();
            w.write_bytes(&[0xAB; 20]) // hash
             .unwrap();
            w.into_bytes()
        };
        let key = StorageKey::new(ContractManagement::ID, {
            let mut k = vec![PREFIX_CONTRACT];
            k.extend_from_slice(&hash.to_bytes());
            k
        });
        cache.add(key, StorageItem::from_bytes(payload.clone()));

        assert!(ContractManagement::is_contract(&cache, &hash));
        let value = cache
            .get(&StorageKey::new(ContractManagement::ID, {
                let mut k = vec![PREFIX_CONTRACT];
                k.extend_from_slice(&hash.to_bytes());
                k
            }))
            .unwrap();
        assert_eq!(value.value_bytes().as_ref(), payload.as_slice());
    }
}

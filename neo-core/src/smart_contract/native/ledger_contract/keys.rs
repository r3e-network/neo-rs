//! Storage key builders shared by the ledger contract and callers.
use super::{PREFIX_BLOCK, PREFIX_BLOCK_HASH, PREFIX_CURRENT_BLOCK, PREFIX_TRANSACTION};
use crate::smart_contract::StorageKey;
use crate::{UInt160, UInt256};

/// Key-building helpers shared across ledger contract call sites.
pub(crate) fn block_hash_storage_key(contract_id: i32, index: u32) -> StorageKey {
    let mut key = Vec::with_capacity(1 + std::mem::size_of::<u32>());
    key.push(PREFIX_BLOCK_HASH);
    key.extend_from_slice(&index.to_le_bytes());
    StorageKey::new(contract_id, key)
}

pub(crate) fn block_storage_key(contract_id: i32, hash: &UInt256) -> StorageKey {
    let mut key = Vec::with_capacity(1 + hash.to_bytes().len());
    key.push(PREFIX_BLOCK);
    key.extend_from_slice(&hash.to_bytes());
    StorageKey::new(contract_id, key)
}

pub(crate) fn transaction_storage_key(contract_id: i32, hash: &UInt256) -> StorageKey {
    let mut key = Vec::with_capacity(1 + hash.to_bytes().len());
    key.push(PREFIX_TRANSACTION);
    key.extend_from_slice(&hash.to_bytes());
    StorageKey::new(contract_id, key)
}

pub(crate) fn transaction_conflict_storage_key(
    contract_id: i32,
    hash: &UInt256,
    signer: &UInt160,
) -> StorageKey {
    let mut key = Vec::with_capacity(1 + hash.to_bytes().len() + signer.to_bytes().len());
    key.push(PREFIX_TRANSACTION);
    key.extend_from_slice(&hash.to_bytes());
    key.extend_from_slice(&signer.to_bytes());
    StorageKey::new(contract_id, key)
}

pub(crate) fn current_block_storage_key(contract_id: i32) -> StorageKey {
    StorageKey::new(contract_id, vec![PREFIX_CURRENT_BLOCK])
}

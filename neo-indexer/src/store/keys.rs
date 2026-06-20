//! Durable key schema for the Neo indexer service store.

use neo_primitives::{UInt160, UInt256};

use crate::model::{NotificationIndexRecord, TransactionIndexRecord};

pub(crate) const LEGACY_STORE_SNAPSHOT_KEY: &[u8] = b"neo-indexer:snapshot:v2";
pub(crate) const STORE_SCHEMA_VERSION: &[u8] = b"3";
pub(crate) const STORE_PREFIX: &[u8] = b"neo-indexer:v3:";
pub(crate) const STORE_SCHEMA_VERSION_KEY: &[u8] = b"neo-indexer:v3:meta:schema-version";
pub(crate) const BLOCK_BY_HEIGHT_PREFIX: &[u8] = b"neo-indexer:v3:block-by-height:";
pub(crate) const BLOCK_BY_HASH_PREFIX: &[u8] = b"neo-indexer:v3:block-by-hash:";
pub(crate) const TRANSACTION_BY_CHAIN_PREFIX: &[u8] = b"neo-indexer:v3:tx-by-chain:";
pub(crate) const TRANSACTION_BY_HASH_PREFIX: &[u8] = b"neo-indexer:v3:tx-by-hash:";
pub(crate) const ACCOUNT_TRANSACTION_PREFIX: &[u8] = b"neo-indexer:v3:account-tx:";
pub(crate) const NOTIFICATION_BY_CHAIN_PREFIX: &[u8] = b"neo-indexer:v3:notification-by-chain:";
pub(crate) const NOTIFICATION_BY_BLOCK_PREFIX: &[u8] = b"neo-indexer:v3:notification-by-block:";
pub(crate) const NOTIFICATION_BY_TRANSACTION_PREFIX: &[u8] = b"neo-indexer:v3:notification-by-tx:";
pub(crate) const NOTIFICATION_BY_CONTRACT_PREFIX: &[u8] =
    b"neo-indexer:v3:notification-by-contract:";
pub(crate) const NOTIFICATION_BY_ACCOUNT_PREFIX: &[u8] = b"neo-indexer:v3:notification-by-account:";

pub(crate) fn block_by_height_key(height: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(BLOCK_BY_HEIGHT_PREFIX.len() + 4);
    key.extend_from_slice(BLOCK_BY_HEIGHT_PREFIX);
    key.extend_from_slice(&height.to_be_bytes());
    key
}

pub(crate) fn block_by_hash_key(hash: &UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(BLOCK_BY_HASH_PREFIX.len() + UInt256::LENGTH);
    key.extend_from_slice(BLOCK_BY_HASH_PREFIX);
    key.extend_from_slice(&hash.to_array());
    key
}

pub(crate) fn transaction_by_chain_key(transaction: &TransactionIndexRecord) -> Vec<u8> {
    let mut key = Vec::with_capacity(TRANSACTION_BY_CHAIN_PREFIX.len() + 4 + 4 + UInt256::LENGTH);
    key.extend_from_slice(TRANSACTION_BY_CHAIN_PREFIX);
    key.extend_from_slice(&transaction.block_height.to_be_bytes());
    key.extend_from_slice(&transaction.transaction_index.to_be_bytes());
    key.extend_from_slice(&transaction.hash.to_array());
    key
}

pub(crate) fn transaction_by_hash_key(hash: &UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(TRANSACTION_BY_HASH_PREFIX.len() + UInt256::LENGTH);
    key.extend_from_slice(TRANSACTION_BY_HASH_PREFIX);
    key.extend_from_slice(&hash.to_array());
    key
}

pub(crate) fn transaction_by_block_prefix(height: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(TRANSACTION_BY_CHAIN_PREFIX.len() + 4);
    key.extend_from_slice(TRANSACTION_BY_CHAIN_PREFIX);
    key.extend_from_slice(&height.to_be_bytes());
    key
}

pub(crate) fn account_transaction_key(
    account: &UInt160,
    transaction: &TransactionIndexRecord,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(
        ACCOUNT_TRANSACTION_PREFIX.len() + UInt160::LENGTH + 4 + 4 + UInt256::LENGTH,
    );
    key.extend_from_slice(ACCOUNT_TRANSACTION_PREFIX);
    key.extend_from_slice(&account.to_array());
    key.extend_from_slice(&transaction.block_height.to_be_bytes());
    key.extend_from_slice(&transaction.transaction_index.to_be_bytes());
    key.extend_from_slice(&transaction.hash.to_array());
    key
}

pub(crate) fn account_transaction_prefix(account: &UInt160) -> Vec<u8> {
    let mut key = Vec::with_capacity(ACCOUNT_TRANSACTION_PREFIX.len() + UInt160::LENGTH);
    key.extend_from_slice(ACCOUNT_TRANSACTION_PREFIX);
    key.extend_from_slice(&account.to_array());
    key
}

pub(crate) fn notification_by_block_prefix(hash: &UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(NOTIFICATION_BY_BLOCK_PREFIX.len() + UInt256::LENGTH);
    key.extend_from_slice(NOTIFICATION_BY_BLOCK_PREFIX);
    key.extend_from_slice(&hash.to_array());
    key
}

pub(crate) fn notification_by_transaction_prefix(tx_hash: &UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(NOTIFICATION_BY_TRANSACTION_PREFIX.len() + UInt256::LENGTH);
    key.extend_from_slice(NOTIFICATION_BY_TRANSACTION_PREFIX);
    key.extend_from_slice(&tx_hash.to_array());
    key
}

pub(crate) fn notification_by_contract_prefix(contract_hash: &UInt160) -> Vec<u8> {
    let mut key = Vec::with_capacity(NOTIFICATION_BY_CONTRACT_PREFIX.len() + UInt160::LENGTH);
    key.extend_from_slice(NOTIFICATION_BY_CONTRACT_PREFIX);
    key.extend_from_slice(&contract_hash.to_array());
    key
}

pub(crate) fn notification_by_account_prefix(account: &UInt160) -> Vec<u8> {
    let mut key = Vec::with_capacity(NOTIFICATION_BY_ACCOUNT_PREFIX.len() + UInt160::LENGTH);
    key.extend_from_slice(NOTIFICATION_BY_ACCOUNT_PREFIX);
    key.extend_from_slice(&account.to_array());
    key
}

fn notification_chain_suffix(notification: &NotificationIndexRecord) -> [u8; 12] {
    let mut suffix = [0u8; 12];
    suffix[0..4].copy_from_slice(&notification.block_height.to_be_bytes());
    suffix[4..8].copy_from_slice(&notification.execution_index.to_be_bytes());
    suffix[8..12].copy_from_slice(&notification.notification_index.to_be_bytes());
    suffix
}

pub(crate) fn notification_by_chain_key(notification: &NotificationIndexRecord) -> Vec<u8> {
    let suffix = notification_chain_suffix(notification);
    let mut key = Vec::with_capacity(NOTIFICATION_BY_CHAIN_PREFIX.len() + suffix.len());
    key.extend_from_slice(NOTIFICATION_BY_CHAIN_PREFIX);
    key.extend_from_slice(&suffix);
    key
}

pub(crate) fn notification_by_block_key(notification: &NotificationIndexRecord) -> Vec<u8> {
    let suffix = notification_chain_suffix(notification);
    let mut key =
        Vec::with_capacity(NOTIFICATION_BY_BLOCK_PREFIX.len() + UInt256::LENGTH + suffix.len());
    key.extend_from_slice(NOTIFICATION_BY_BLOCK_PREFIX);
    key.extend_from_slice(&notification.block_hash.to_array());
    key.extend_from_slice(&suffix);
    key
}

pub(crate) fn notification_by_transaction_key(
    tx_hash: &UInt256,
    notification: &NotificationIndexRecord,
) -> Vec<u8> {
    let suffix = notification_chain_suffix(notification);
    let mut key = Vec::with_capacity(
        NOTIFICATION_BY_TRANSACTION_PREFIX.len() + UInt256::LENGTH + suffix.len(),
    );
    key.extend_from_slice(NOTIFICATION_BY_TRANSACTION_PREFIX);
    key.extend_from_slice(&tx_hash.to_array());
    key.extend_from_slice(&suffix);
    key
}

pub(crate) fn notification_by_contract_key(notification: &NotificationIndexRecord) -> Vec<u8> {
    let suffix = notification_chain_suffix(notification);
    let mut key =
        Vec::with_capacity(NOTIFICATION_BY_CONTRACT_PREFIX.len() + UInt160::LENGTH + suffix.len());
    key.extend_from_slice(NOTIFICATION_BY_CONTRACT_PREFIX);
    key.extend_from_slice(&notification.contract_hash.to_array());
    key.extend_from_slice(&suffix);
    key
}

pub(crate) fn notification_by_account_key(
    account: &UInt160,
    notification: &NotificationIndexRecord,
) -> Vec<u8> {
    let suffix = notification_chain_suffix(notification);
    let mut key =
        Vec::with_capacity(NOTIFICATION_BY_ACCOUNT_PREFIX.len() + UInt160::LENGTH + suffix.len());
    key.extend_from_slice(NOTIFICATION_BY_ACCOUNT_PREFIX);
    key.extend_from_slice(&account.to_array());
    key.extend_from_slice(&suffix);
    key
}

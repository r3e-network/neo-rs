//! # neo-indexer::store
//!
//! Durable indexer store encoding, key layout, and migration helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-indexer`. This service crate owns projections
//! over committed chain data and must not decide block validity or consensus
//! outcomes.
//!
//! ## Contents
//!
//! - `keys`: indexer store key builders and key-prefix constants.
//! - `lifecycle`: store-backed indexer lifecycle, migration, and delta writes.
//! - `records`: encoded indexer record pages and lookup helpers.
//! - `status`: store status summary helpers.

mod keys;
mod lifecycle;
mod records;
mod status;

pub(crate) use keys::{
    BLOCK_BY_HEIGHT_PREFIX, account_transaction_prefix, block_by_hash_key, block_by_height_key,
    notification_by_account_prefix, notification_by_block_prefix, notification_by_contract_prefix,
    notification_by_transaction_prefix, transaction_by_block_prefix, transaction_by_hash_key,
};
pub(crate) use lifecycle::{read_indexer, write_indexer_delta};
pub(crate) use records::{
    get_record, read_record_page, read_record_page_filtered, read_record_prefix_filtered,
};
pub(crate) use status::status;

#[cfg(test)]
pub(crate) use keys::{
    ACCOUNT_TRANSACTION_PREFIX, BLOCK_BY_HASH_PREFIX, LEGACY_STORE_SNAPSHOT_KEY,
    NOTIFICATION_BY_ACCOUNT_PREFIX, NOTIFICATION_BY_BLOCK_PREFIX, NOTIFICATION_BY_CHAIN_PREFIX,
    NOTIFICATION_BY_CONTRACT_PREFIX, NOTIFICATION_BY_TRANSACTION_PREFIX,
    TRANSACTION_BY_CHAIN_PREFIX, TRANSACTION_BY_HASH_PREFIX, account_transaction_key,
};

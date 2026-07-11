//! # neo-indexer::store
//!
//! Durable indexer service-store encoding, key layout, and query helpers.
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
//! - `lifecycle`: store-backed indexer open and atomic delta writes.
//! - `record_codec`: JSON encoding and decoding for store records.
//! - `record_read`: encoded record lookup, paging, and filtered reads.
//! - `record_write`: projection-to-record materialization.

mod keys;
mod lifecycle;
mod record_codec;
mod record_read;
mod record_write;

#[cfg(test)]
pub(crate) use keys::{
    ACCOUNT_TRANSACTION_PREFIX, BLOCK_BY_HASH_PREFIX, LEGACY_STORE_SNAPSHOT_KEY,
    NOTIFICATION_BY_ACCOUNT_PREFIX, NOTIFICATION_BY_BLOCK_PREFIX, NOTIFICATION_BY_CHAIN_PREFIX,
    NOTIFICATION_BY_CONTRACT_PREFIX, NOTIFICATION_BY_TRANSACTION_PREFIX,
    TRANSACTION_BY_CHAIN_PREFIX, TRANSACTION_BY_HASH_PREFIX, account_transaction_key,
};
pub(crate) use keys::{
    BLOCK_BY_HEIGHT_PREFIX, account_transaction_prefix, block_by_hash_key, block_by_height_key,
    notification_by_account_prefix, notification_by_block_prefix, notification_by_contract_prefix,
    notification_by_transaction_prefix, transaction_by_block_prefix, transaction_by_hash_key,
};
pub(crate) use lifecycle::{read_indexer, write_indexer_change_set, write_indexer_delta};
pub(crate) use record_read::{
    get_record, read_record_page, read_record_page_filtered, read_record_prefix_filtered,
};

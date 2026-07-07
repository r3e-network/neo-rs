//! # neo-indexer::indexer
//!
//! Indexer workers and projection logic for chain-derived data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-indexer`. This service crate owns projections
//! over committed chain data and must not decide block validity or consensus
//! outcomes.
//!
//! ## Contents
//!
//! - `apply`: prepared block and notification application into indexes.
//! - `block`: block and transaction materialization before indexing.
//! - `commands`: public block and notification indexing commands.
//! - `notifications`: notification projection and query logic.
//! - `query`: query APIs for indexed data.
//! - `reorg`: reorg-aware index update helpers.
//! - `snapshot`: Read snapshot view for the surrounding store backend.
//! - `tests`: Module-local tests and regression coverage.

mod apply;
mod block;
mod commands;
mod notifications;
mod query;
mod reorg;
mod snapshot;

use std::collections::{BTreeMap, HashMap};

use neo_primitives::{UInt160, UInt256};

#[cfg(test)]
use crate::error::IndexerError;
use crate::model::{
    AccountTransactionRecord, BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord,
};

/// Mutable in-memory index over canonical blocks and transactions.
#[derive(Debug, Default)]
pub struct Indexer {
    blocks_by_hash: HashMap<UInt256, BlockIndexRecord>,
    block_hash_by_height: BTreeMap<u32, UInt256>,
    transactions_by_hash: HashMap<UInt256, TransactionIndexRecord>,
    tx_hashes_by_block: HashMap<UInt256, Vec<UInt256>>,
    account_transactions: HashMap<UInt160, Vec<AccountTransactionRecord>>,
    account_notifications: HashMap<UInt160, Vec<NotificationIndexRecord>>,
    notifications: Vec<NotificationIndexRecord>,
}

impl Indexer {
    /// Creates an empty index.
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
#[path = "../tests/indexer/mod.rs"]
mod tests;

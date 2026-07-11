//! # neo-indexer::indexer::change_set
//!
//! Block-scoped projection changes shared by memory and service-store writes.
//!
//! ## Boundary
//!
//! This module plans records derived from already validated canonical blocks.
//! It does not commit storage or decide chain validity.
//!
//! ## Contents
//!
//! - `IndexedBlockBundle`: one block and all primary derived records.
//! - `ProjectionChangeSet`: removed and inserted bundles for one atomic command.

use std::collections::HashSet;

use neo_primitives::UInt256;

use super::Indexer;
use super::commands::{PreparedBatchEntry, PreparedIndexBatch};
use crate::error::{IndexerError, IndexerResult};
use crate::model::{BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord};

/// All primary records derived from one canonical block.
#[derive(Debug, Clone)]
pub(crate) struct IndexedBlockBundle {
    pub(crate) block: BlockIndexRecord,
    pub(crate) transactions: Vec<TransactionIndexRecord>,
    pub(crate) notifications: Vec<NotificationIndexRecord>,
}

/// Records removed and inserted by one atomic projection command.
pub(crate) struct ProjectionChangeSet {
    pub(crate) removed: Vec<IndexedBlockBundle>,
    pub(crate) inserted: Vec<IndexedBlockBundle>,
}

impl ProjectionChangeSet {
    pub(crate) fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.inserted.is_empty()
    }
}

impl Indexer {
    pub(crate) fn validate_prepared_entries(
        &self,
        entries: &[PreparedBatchEntry],
    ) -> IndexerResult<()> {
        let removed_hashes = self.affected_block_hashes(entries);
        let removed_transactions = removed_hashes
            .iter()
            .filter_map(|hash| self.tx_hashes_by_block.get(hash))
            .flatten()
            .copied()
            .collect::<HashSet<_>>();

        for entry in entries {
            for transaction in &entry.block.transactions {
                if self.transactions_by_hash.contains_key(&transaction.hash)
                    && !removed_transactions.contains(&transaction.hash)
                {
                    return Err(IndexerError::DuplicateTransaction {
                        hash: transaction.hash,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn projection_change_set(
        &self,
        prepared: &PreparedIndexBatch,
    ) -> IndexerResult<ProjectionChangeSet> {
        self.validate_prepared_entries(&prepared.entries)?;

        let mut removed = self
            .affected_block_hashes(&prepared.entries)
            .into_iter()
            .filter_map(|hash| self.block_bundle(&hash))
            .collect::<Vec<_>>();
        removed.sort_by_key(|bundle| bundle.block.height);
        let inserted = prepared
            .entries
            .iter()
            .map(IndexedBlockBundle::from)
            .collect();

        Ok(ProjectionChangeSet { removed, inserted })
    }

    fn affected_block_hashes(&self, entries: &[PreparedBatchEntry]) -> HashSet<UInt256> {
        let mut hashes = HashSet::with_capacity(entries.len().saturating_mul(2));
        for entry in entries {
            if let Some(hash) = self
                .block_hash_by_height
                .get(&entry.block.block.height)
                .copied()
            {
                hashes.insert(hash);
            }
            if self.blocks_by_hash.contains_key(&entry.block.block.hash) {
                hashes.insert(entry.block.block.hash);
            }
        }
        hashes
    }

    fn block_bundle(&self, hash: &UInt256) -> Option<IndexedBlockBundle> {
        let block = self.blocks_by_hash.get(hash)?.clone();
        let transactions = self
            .tx_hashes_by_block
            .get(hash)
            .into_iter()
            .flatten()
            .filter_map(|tx_hash| self.transactions_by_hash.get(tx_hash))
            .cloned()
            .collect();
        let notifications = self
            .notifications
            .iter()
            .filter(|notification| notification.block_hash == *hash)
            .cloned()
            .collect();
        Some(IndexedBlockBundle {
            block,
            transactions,
            notifications,
        })
    }
}

impl From<&PreparedBatchEntry> for IndexedBlockBundle {
    fn from(entry: &PreparedBatchEntry) -> Self {
        Self {
            block: entry.block.block.clone(),
            transactions: entry.block.transactions.clone(),
            notifications: entry.notifications.clone(),
        }
    }
}

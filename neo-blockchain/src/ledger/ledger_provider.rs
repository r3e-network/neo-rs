//! # neo-blockchain::ledger::ledger_provider
//!
//! Provider-style read API over persisted Neo ledger records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. It owns read-only ledger
//! capabilities over hot native Ledger records and cold provider-compatible
//! archives, but it does not persist new blocks or choose pruning policy.
//!
//! ## Contents
//!
//! - `BlockProvider`: block/header/hash read capability.
//! - `TxProvider`: transaction read capability.
//! - `StorageLedgerProvider`: hot provider over native Ledger records in a
//!   `DataCache`.

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Header, Transaction};
use neo_primitives::UInt256;
use neo_storage::DataCache;

/// Read-only access to persisted block records.
pub trait BlockProvider {
    /// Returns the canonical block hash stored for `index`.
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>>;

    /// Returns the persisted block header stored under `hash`.
    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>>;

    /// Returns the full block stored under `hash`.
    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>>;

    /// Returns the full block stored at `index`.
    fn block_by_index(&self, index: u32) -> CoreResult<Option<Block>> {
        let Some(hash) = self.block_hash_by_index(index)? else {
            return Ok(None);
        };
        self.block_by_hash(&hash)
    }

    /// Returns the persisted block header stored at `index`.
    fn header_by_index(&self, index: u32) -> CoreResult<Option<Header>> {
        let Some(hash) = self.block_hash_by_index(index)? else {
            return Ok(None);
        };
        self.header_by_hash(&hash)
    }

    /// Returns the block height for `hash`.
    fn block_index_by_hash(&self, hash: &UInt256) -> CoreResult<Option<u32>> {
        Ok(self.header_by_hash(hash)?.map(|header| header.index()))
    }
}

impl<P> BlockProvider for &P
where
    P: BlockProvider + ?Sized,
{
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        (**self).block_hash_by_index(index)
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        (**self).header_by_hash(hash)
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        (**self).block_by_hash(hash)
    }
}

/// Read-only access to persisted transaction records.
pub trait TxProvider {
    /// Returns the persisted transaction stored under `hash`.
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>>;

    /// Returns whether a persisted transaction record exists for `hash`.
    fn contains_transaction(&self, hash: &UInt256) -> CoreResult<bool> {
        Ok(self.transaction_by_hash(hash)?.is_some())
    }
}

impl<P> TxProvider for &P
where
    P: TxProvider + ?Sized,
{
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        (**self).transaction_by_hash(hash)
    }
}

/// Storage-backed provider over Neo ledger native-contract records.
pub struct StorageLedgerProvider<'a> {
    snapshot: &'a DataCache,
}

impl<'a> StorageLedgerProvider<'a> {
    /// Creates a provider over `snapshot`.
    pub const fn new(snapshot: &'a DataCache) -> Self {
        Self { snapshot }
    }
}

impl BlockProvider for StorageLedgerProvider<'_> {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        LedgerContract::new().get_block_hash(self.snapshot, index)
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(LedgerContract::new()
            .get_trimmed_block(self.snapshot, hash)?
            .map(|trimmed| trimmed.header))
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        let ledger = LedgerContract::new();
        let Some(trimmed) = ledger.get_trimmed_block(self.snapshot, hash)? else {
            return Ok(None);
        };

        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let transaction = ledger
                .get_transaction_state(self.snapshot, tx_hash)?
                .and_then(|state| state.transaction)
                .ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "block {hash} references transaction {tx_hash} with no ledger record"
                    ))
                })?;
            transactions.push(transaction);
        }

        Ok(Some(Block::from_parts(trimmed.header, transactions)))
    }
}

impl TxProvider for StorageLedgerProvider<'_> {
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(LedgerContract::new()
            .get_transaction_state(self.snapshot, hash)?
            .and_then(|state| state.transaction))
    }
}

#[cfg(test)]
#[path = "../tests/ledger/ledger_provider.rs"]
mod tests;

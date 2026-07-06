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
//! - `LedgerProviderFactory`: typed provider construction from a store
//!   snapshot.
//! - `StorageLedgerProvider`: hot provider over native Ledger records in a
//!   `DataCache`.
//! - `HotColdLedgerProvider`: read router that falls back to a cold provider
//!   only when hot native Ledger records miss.

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Header, Transaction};
use neo_primitives::UInt256;
use neo_storage::DataCache;

/// Combined read capability for ledger providers.
pub trait LedgerProvider: BlockProvider + TxProvider {}

impl<P> LedgerProvider for P where P: BlockProvider + TxProvider {}

/// Factory for immutable ledger providers over a specific snapshot.
///
/// The associated provider type keeps this boundary monomorphized for hot
/// paths while still letting composition roots swap in routed providers.
pub trait LedgerProviderFactory {
    /// Provider returned for a snapshot lifetime.
    type Provider<'a>: LedgerProvider + 'a
    where
        Self: 'a;

    /// Creates a read provider over `snapshot`.
    fn provider<'a>(&'a self, snapshot: &'a DataCache) -> Self::Provider<'a>;
}

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

/// Factory for hot native Ledger-record providers.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageLedgerProviderFactory;

impl LedgerProviderFactory for StorageLedgerProviderFactory {
    type Provider<'a> = StorageLedgerProvider<'a>;

    fn provider<'a>(&'a self, snapshot: &'a DataCache) -> Self::Provider<'a> {
        StorageLedgerProvider::new(snapshot)
    }
}

/// Provider that reads hot native Ledger records first and falls back to cold
/// immutable storage only on a miss.
///
/// Errors from the hot provider are returned immediately: a corrupt or
/// unreadable hot store must not be hidden behind old cold data.
#[derive(Clone, Debug)]
pub struct HotColdLedgerProvider<Hot, Cold> {
    hot: Hot,
    cold: Cold,
}

impl<Hot, Cold> HotColdLedgerProvider<Hot, Cold> {
    /// Creates a routed provider from hot and cold providers.
    #[must_use]
    pub const fn new(hot: Hot, cold: Cold) -> Self {
        Self { hot, cold }
    }

    /// Returns the hot provider.
    #[must_use]
    pub const fn hot(&self) -> &Hot {
        &self.hot
    }

    /// Returns the cold provider.
    #[must_use]
    pub const fn cold(&self) -> &Cold {
        &self.cold
    }
}

impl<Hot, Cold> BlockProvider for HotColdLedgerProvider<Hot, Cold>
where
    Hot: BlockProvider,
    Cold: BlockProvider,
{
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        match self.hot.block_hash_by_index(index)? {
            Some(hash) => Ok(Some(hash)),
            None => self.cold.block_hash_by_index(index),
        }
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        match self.hot.header_by_hash(hash)? {
            Some(header) => Ok(Some(header)),
            None => self.cold.header_by_hash(hash),
        }
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        match self.hot.block_by_hash(hash)? {
            Some(block) => Ok(Some(block)),
            None => self.cold.block_by_hash(hash),
        }
    }
}

impl<Hot, Cold> TxProvider for HotColdLedgerProvider<Hot, Cold>
where
    Hot: TxProvider,
    Cold: TxProvider,
{
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        match self.hot.transaction_by_hash(hash)? {
            Some(transaction) => Ok(Some(transaction)),
            None => self.cold.transaction_by_hash(hash),
        }
    }
}

/// Factory that combines hot native Ledger reads with a cold immutable provider.
#[derive(Clone, Debug)]
pub struct HotColdLedgerProviderFactory<Cold> {
    cold: Cold,
}

impl<Cold> HotColdLedgerProviderFactory<Cold> {
    /// Creates a hot/cold factory.
    #[must_use]
    pub const fn new(cold: Cold) -> Self {
        Self { cold }
    }

    /// Returns the cold provider used by this factory.
    #[must_use]
    pub const fn cold(&self) -> &Cold {
        &self.cold
    }
}

impl<Cold> LedgerProviderFactory for HotColdLedgerProviderFactory<Cold>
where
    Cold: LedgerProvider + Clone,
{
    type Provider<'a>
        = HotColdLedgerProvider<StorageLedgerProvider<'a>, Cold>
    where
        Self: 'a;

    fn provider<'a>(&'a self, snapshot: &'a DataCache) -> Self::Provider<'a> {
        HotColdLedgerProvider::new(StorageLedgerProvider::new(snapshot), self.cold.clone())
    }
}

#[cfg(test)]
#[path = "../tests/ledger/ledger_provider.rs"]
mod tests;

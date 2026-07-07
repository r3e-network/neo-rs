//! Ledger provider capability traits.
//!
//! These traits keep ledger reads capability-oriented: consumers depend on the
//! smallest read surface they need, while composition roots can choose a hot,
//! cold, or routed provider implementation.

use neo_error::CoreResult;
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

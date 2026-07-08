//! Hot/cold ledger provider routing.
//!
//! The router prefers hot native Ledger records and only falls back to immutable
//! cold data on clean misses. Hot read errors are returned immediately so
//! corrupt or unreadable live state is never hidden behind older archive data.

use neo_error::CoreResult;
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;

use super::{
    BlockProvider, ChainTipProvider, LedgerProvider, LedgerProviderFactory, StorageLedgerProvider,
    TransactionStateProvider, TxProvider,
};

/// Provider that reads hot native Ledger records first and falls back to cold
/// immutable storage only on a miss.
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

impl<Hot, Cold> TransactionStateProvider for HotColdLedgerProvider<Hot, Cold>
where
    Hot: TransactionStateProvider,
    Cold: TransactionStateProvider,
{
    fn transaction_state_by_hash(&self, hash: &UInt256) -> CoreResult<Option<TransactionState>> {
        match self.hot.transaction_state_by_hash(hash)? {
            Some(state) => Ok(Some(state)),
            None => self.cold.transaction_state_by_hash(hash),
        }
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        if self
            .hot
            .contains_conflict_hash(hash, signers, max_traceable_blocks)?
        {
            return Ok(true);
        }
        self.cold
            .contains_conflict_hash(hash, signers, max_traceable_blocks)
    }
}

impl<Hot, Cold> ChainTipProvider for HotColdLedgerProvider<Hot, Cold>
where
    Hot: ChainTipProvider,
{
    fn current_hash(&self) -> CoreResult<UInt256> {
        self.hot.current_hash()
    }

    fn current_index(&self) -> CoreResult<u32> {
        self.hot.current_index()
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

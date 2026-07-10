//! Empty ledger provider for nodes without a cold archive.
//!
//! This provider is the explicit "no cold tier installed" implementation. It
//! lets composition roots keep the same provider/factory shape while static
//! files or another immutable archive are absent.

use neo_error::CoreResult;
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};

use super::{BlockProvider, LedgerProviderFactory, TransactionStateProvider, TxProvider};

/// Ledger provider that always reports clean misses.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyLedgerProvider;

impl BlockProvider for EmptyLedgerProvider {
    fn block_hash_by_index(&self, _index: u32) -> CoreResult<Option<UInt256>> {
        Ok(None)
    }

    fn header_by_hash(&self, _hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(None)
    }

    fn block_by_hash(&self, _hash: &UInt256) -> CoreResult<Option<Block>> {
        Ok(None)
    }
}

impl TxProvider for EmptyLedgerProvider {
    fn transaction_by_hash(&self, _hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(None)
    }
}

impl TransactionStateProvider for EmptyLedgerProvider {
    fn transaction_state_by_hash(&self, _hash: &UInt256) -> CoreResult<Option<TransactionState>> {
        Ok(None)
    }

    fn contains_conflict_hash(
        &self,
        _hash: &UInt256,
        _signers: &[UInt160],
        _max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        Ok(false)
    }
}

/// Factory for [`EmptyLedgerProvider`].
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyLedgerProviderFactory;

impl LedgerProviderFactory for EmptyLedgerProviderFactory {
    type Provider<'a, B>
        = EmptyLedgerProvider
    where
        B: CacheRead;

    fn provider<'a, B: CacheRead>(&'a self, _snapshot: &'a DataCache<B>) -> Self::Provider<'a, B> {
        EmptyLedgerProvider
    }
}

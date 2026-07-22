//! Adapter from a snapshot-bound blockchain ledger provider to mempool reads.

use neo_mempool::AdmissionLedgerProvider;
use neo_storage::{CacheRead, DataCache};

use super::{ChainTipProvider, TransactionStateProvider, TxProvider};

/// Reuses the configured hot/cold ledger routing for transaction admission.
#[derive(Debug, Clone, Copy)]
pub struct TransactionAdmissionLedger<P> {
    provider: P,
}

impl<P> TransactionAdmissionLedger<P> {
    /// Wrap a snapshot-bound blockchain ledger provider.
    pub const fn new(provider: P) -> Self {
        Self { provider }
    }
}

impl<P> AdmissionLedgerProvider for TransactionAdmissionLedger<P>
where
    P: ChainTipProvider + TxProvider + TransactionStateProvider,
{
    fn current_index<B: CacheRead>(&self, _snapshot: &DataCache<B>) -> neo_error::CoreResult<u32> {
        self.provider.current_index()
    }

    fn contains_transaction<B: CacheRead>(
        &self,
        _snapshot: &DataCache<B>,
        hash: &neo_primitives::UInt256,
    ) -> neo_error::CoreResult<bool> {
        self.provider.contains_transaction(hash)
    }

    fn contains_conflict_hash<B: CacheRead>(
        &self,
        _snapshot: &DataCache<B>,
        hash: &neo_primitives::UInt256,
        signers: &[neo_primitives::UInt160],
        max_traceable_blocks: u32,
    ) -> neo_error::CoreResult<bool> {
        self.provider
            .contains_conflict_hash(hash, signers, max_traceable_blocks)
    }
}

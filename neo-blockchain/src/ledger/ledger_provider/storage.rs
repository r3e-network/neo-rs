//! Hot ledger provider backed by native Ledger contract storage records.

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};

use super::{
    BlockProvider, ChainTipProvider, LedgerProviderFactory, TransactionStateProvider, TxProvider,
};

/// Storage-backed provider over Neo ledger native-contract records.
pub struct StorageLedgerProvider<'a, B: CacheRead> {
    snapshot: &'a DataCache<B>,
    ledger: LedgerContract,
}

impl<'a, B: CacheRead> StorageLedgerProvider<'a, B> {
    /// Creates a provider over `snapshot`.
    pub const fn new(snapshot: &'a DataCache<B>) -> Self {
        Self {
            snapshot,
            ledger: LedgerContract::new(),
        }
    }

    /// Returns the optional canonical tip while distinguishing an absent
    /// uninitialized pointer from malformed persisted bytes.
    pub fn optional_current_tip(&self) -> CoreResult<Option<(UInt256, u32)>> {
        self.ledger.optional_current_tip(self.snapshot)
    }
}

impl<B: CacheRead> BlockProvider for StorageLedgerProvider<'_, B> {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        self.ledger.get_block_hash(self.snapshot, index)
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(self
            .ledger
            .get_trimmed_block(self.snapshot, hash)?
            .map(|trimmed| trimmed.header))
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        let Some(trimmed) = self.ledger.get_trimmed_block(self.snapshot, hash)? else {
            return Ok(None);
        };

        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let transaction = self
                .ledger
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

impl<B: CacheRead> ChainTipProvider for StorageLedgerProvider<'_, B> {
    fn current_hash(&self) -> CoreResult<UInt256> {
        self.ledger.current_hash(self.snapshot)
    }

    fn current_index(&self) -> CoreResult<u32> {
        self.ledger.current_index(self.snapshot)
    }
}

impl<B: CacheRead> TxProvider for StorageLedgerProvider<'_, B> {
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(self
            .ledger
            .get_transaction_state(self.snapshot, hash)?
            .and_then(|state| state.transaction))
    }
}

impl<B: CacheRead> TransactionStateProvider for StorageLedgerProvider<'_, B> {
    fn transaction_state_by_hash(&self, hash: &UInt256) -> CoreResult<Option<TransactionState>> {
        self.ledger.get_transaction_state(self.snapshot, hash)
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        self.ledger
            .contains_conflict_hash(self.snapshot, hash, signers, max_traceable_blocks)
    }
}

/// Factory for hot native Ledger-record providers.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageLedgerProviderFactory;

impl LedgerProviderFactory for StorageLedgerProviderFactory {
    type Provider<'a, B>
        = StorageLedgerProvider<'a, B>
    where
        B: CacheRead;

    fn provider<'a, B: CacheRead>(&'a self, snapshot: &'a DataCache<B>) -> Self::Provider<'a, B> {
        StorageLedgerProvider::new(snapshot)
    }
}

//! Hot ledger provider backed by native Ledger contract storage records.

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Header, Transaction};
use neo_primitives::UInt256;
use neo_storage::DataCache;

use super::{BlockProvider, ChainTipProvider, LedgerProviderFactory, TxProvider};

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

impl ChainTipProvider for StorageLedgerProvider<'_> {
    fn current_hash(&self) -> CoreResult<UInt256> {
        LedgerContract::new().current_hash(self.snapshot)
    }

    fn current_index(&self) -> CoreResult<u32> {
        LedgerContract::new().current_index(self.snapshot)
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

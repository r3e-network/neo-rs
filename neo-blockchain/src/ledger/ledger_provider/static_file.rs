//! Cold Ledger provider backed by finalized append-only records.

use neo_error::{CoreError, CoreResult};
use neo_io::{MemoryReader, Serializable};
use neo_native_contracts::LedgerContract;
use neo_native_contracts::ledger_contract::storage::{
    PREFIX_BLOCK, PREFIX_BLOCK_HASH, PREFIX_TRANSACTION,
};
use neo_payloads::{Block, Header, Transaction, TransactionState, TrimmedBlock};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache, StorageKey};

use crate::ledger::static_archive::StaticLedgerArchive;

use super::{BlockProvider, LedgerProviderFactory, TransactionStateProvider, TxProvider};

/// Read-only Ledger capabilities over finalized static files.
#[derive(Clone, Debug)]
pub struct StaticLedgerProvider {
    archive: StaticLedgerArchive,
}

impl StaticLedgerProvider {
    /// Creates a provider over one static Ledger archive.
    #[must_use]
    pub const fn new(archive: StaticLedgerArchive) -> Self {
        Self { archive }
    }

    /// Returns the underlying archive.
    #[must_use]
    pub const fn archive(&self) -> &StaticLedgerArchive {
        &self.archive
    }

    fn block_hash_key(index: u32) -> StorageKey {
        StorageKey::create_with_uint32(LedgerContract::ID, PREFIX_BLOCK_HASH, index)
    }

    fn block_key(hash: &UInt256) -> StorageKey {
        StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_BLOCK, hash)
    }

    fn transaction_key(hash: &UInt256) -> StorageKey {
        StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_TRANSACTION, hash)
    }

    fn conflict_signer_key(hash: &UInt256, signer: &UInt160) -> StorageKey {
        StorageKey::create_with_uint256_uint160(
            LedgerContract::ID,
            PREFIX_TRANSACTION,
            hash,
            signer,
        )
    }

    fn transaction_state_for_key(&self, key: &StorageKey) -> CoreResult<Option<TransactionState>> {
        self.archive
            .get(key)?
            .map(|bytes| LedgerContract::decode_transaction_state(&bytes))
            .transpose()
    }
}

impl BlockProvider for StaticLedgerProvider {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        self.archive
            .get(&Self::block_hash_key(index))?
            .map(|bytes| {
                UInt256::from_bytes(&bytes).map_err(|error| {
                    CoreError::invalid_data(format!(
                        "static Ledger block hash at height {index}: {error}"
                    ))
                })
            })
            .transpose()
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(self.trimmed_block(hash)?.map(|block| block.header))
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        let Some(trimmed) = self.trimmed_block(hash)? else {
            return Ok(None);
        };
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for transaction_hash in &trimmed.hashes {
            let transaction = self
                .transaction_state_by_hash(transaction_hash)?
                .and_then(|state| state.transaction)
                .ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "static Ledger block {hash} references transaction {transaction_hash} without a full record"
                    ))
                })?;
            transactions.push(transaction);
        }
        Ok(Some(Block::from_parts(trimmed.header, transactions)))
    }
}

impl StaticLedgerProvider {
    fn trimmed_block(&self, hash: &UInt256) -> CoreResult<Option<TrimmedBlock>> {
        self.archive
            .get(&Self::block_key(hash))?
            .map(|bytes| {
                let mut reader = MemoryReader::new(&bytes);
                let block = TrimmedBlock::deserialize(&mut reader).map_err(|error| {
                    CoreError::invalid_data(format!("static Ledger trimmed block {hash}: {error}"))
                })?;
                if reader.remaining() != 0 {
                    return Err(CoreError::invalid_data(format!(
                        "static Ledger trimmed block {hash} has {} trailing bytes",
                        reader.remaining()
                    )));
                }
                Ok(block)
            })
            .transpose()
    }
}

impl TxProvider for StaticLedgerProvider {
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(self
            .transaction_state_by_hash(hash)?
            .and_then(|state| state.transaction))
    }
}

impl TransactionStateProvider for StaticLedgerProvider {
    fn transaction_state_by_hash(&self, hash: &UInt256) -> CoreResult<Option<TransactionState>> {
        self.transaction_state_for_key(&Self::transaction_key(hash))
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        let Some(current) = self.archive.tip() else {
            return Ok(false);
        };
        let Some(stub) = self.transaction_state_by_hash(hash)? else {
            return Ok(false);
        };
        if stub.transaction.is_some()
            || !LedgerContract::is_within_trace_window(
                stub.block_index,
                current,
                max_traceable_blocks,
            )
        {
            return Ok(false);
        }
        for signer in signers {
            let Some(state) =
                self.transaction_state_for_key(&Self::conflict_signer_key(hash, signer))?
            else {
                continue;
            };
            if LedgerContract::is_within_trace_window(
                state.block_index,
                current,
                max_traceable_blocks,
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Factory that supplies one cloneable static Ledger provider.
#[derive(Clone, Debug)]
pub struct StaticLedgerProviderFactory {
    provider: StaticLedgerProvider,
}

impl StaticLedgerProviderFactory {
    /// Creates a factory for `archive`.
    #[must_use]
    pub fn new(archive: StaticLedgerArchive) -> Self {
        Self {
            provider: StaticLedgerProvider::new(archive),
        }
    }
}

impl LedgerProviderFactory for StaticLedgerProviderFactory {
    type Provider<'a, B>
        = StaticLedgerProvider
    where
        B: CacheRead;

    fn provider<'a, B: CacheRead>(&'a self, _snapshot: &'a DataCache<B>) -> Self::Provider<'a, B> {
        self.provider.clone()
    }
}

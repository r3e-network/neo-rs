//
// transaction.rs - Transaction-related methods for Blockchain actor
//

use super::*;
use neo_mempool::{AdmissionChainPort, ChainQuery, ChainQueryError, ChainQueryResult};

/// The node's persisted chain state, as mempool admission asks for it.
///
/// Admission decides *whether* a transaction may enter the pool. This view only
/// answers the questions admission asks — the snapshot to verify and insert
/// against, the chain height for diagnostics, and the two persisted-state
/// lookups — over one store snapshot, so every gate and the insertion itself see
/// the same chain.
pub(super) struct PersistedChain<'a> {
    store_cache: &'a StoreCache,
    settings: &'a ProtocolSettings,
    ledger_contract: LedgerContract,
}

impl<'a> PersistedChain<'a> {
    /// Builds the view over one store snapshot and its protocol settings.
    pub(super) fn new(store_cache: &'a StoreCache, settings: &'a ProtocolSettings) -> Self {
        Self {
            store_cache,
            settings,
            ledger_contract: LedgerContract::new(),
        }
    }
}

impl AdmissionChainPort for PersistedChain<'_> {
    fn snapshot(&self) -> &PersistenceDataCache {
        self.store_cache.data_cache()
    }

    fn chain_height(&self) -> u32 {
        self.ledger_contract
            .current_index(self.store_cache)
            .unwrap_or(0)
    }

    fn contains_transaction(&self, hash: &UInt256) -> ChainQueryResult<bool> {
        self.ledger_contract
            .contains_transaction(self.store_cache, hash)
            .map_err(|error| {
                ChainQueryError::new(ChainQuery::TransactionExistence, error.to_string())
            })
    }

    fn max_traceable_blocks(&self) -> u32 {
        self.ledger_contract
            .max_traceable_blocks_snapshot(self.store_cache, self.settings)
            .unwrap_or(self.settings.max_traceable_blocks)
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> ChainQueryResult<bool> {
        self.ledger_contract
            .contains_conflict_hash(self.store_cache, hash, signers, max_traceable_blocks)
            .map_err(|error| ChainQueryError::new(ChainQuery::ConflictExistence, error.to_string()))
    }
}

impl Blockchain {
    /// Admits a relayed transaction into the memory pool.
    ///
    /// The admission policy itself (pool membership, on-chain presence, persisted
    /// conflicts, then the pool's own checks) is owned by
    /// `neo_mempool::admit_transaction`; the actor only supplies the runtime pool
    /// handle and the chain-state view and returns the verdict.
    #[tracing::instrument(name = "blockchain_new_tx", skip(self, transaction), fields(tx_hash = tracing::field::Empty))]
    pub(super) fn on_new_transaction(&self, transaction: &Transaction) -> VerifyResult {
        let span = tracing::Span::current();
        if let Ok(hash) = transaction.try_hash() {
            span.record("tx_hash", tracing::field::display(&hash));
        }
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let store_cache = context.store_cache();
        let settings = context.protocol_settings();
        let chain = PersistedChain::new(&store_cache, &settings);
        let memory_pool = context.memory_pool_handle();
        let mut pool = memory_pool.lock();
        neo_mempool::admit_transaction(&mut pool, transaction, &chain, &settings)
    }
}

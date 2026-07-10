//! Store-backed block source for local ledger mode.

use std::sync::Arc;

use neo_blockchain::{
    BlockProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
    TxProvider,
};

const LOCAL_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

/// Read-only ledger view that serves peers' block requests
/// ([`neo_network::BlockSource`]) by reconstructing a full block from the
/// persistent store: `index -> hash -> TrimmedBlock -> transactions`
/// (the C# `NativeContract.Ledger.GetBlock(snapshot, index)` path).
pub(in crate::node) struct LedgerBlockSource<
    B: neo_storage::CacheRead = neo_storage::EmptyCacheBacking,
> {
    snapshot: Arc<neo_storage::persistence::DataCache<B>>,
    /// Blockchain relay cache for accepted extensible payloads (dBFT and
    /// state-service messages).
    ledger: Arc<neo_blockchain::LedgerContext>,
    /// The shared mempool, so `Inv`/`Mempool` gossip can answer for
    /// unconfirmed transactions (which are not yet in the ledger snapshot).
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl<B: neo_storage::CacheRead> LedgerBlockSource<B> {
    pub(in crate::node) fn new(
        snapshot: Arc<neo_storage::persistence::DataCache<B>>,
        ledger: Arc<neo_blockchain::LedgerContext>,
        mempool: Arc<neo_mempool::MemoryPool>,
    ) -> Self {
        Self {
            snapshot,
            ledger,
            mempool,
        }
    }

    /// Creates the canonical durable ledger provider for this snapshot.
    fn persisted_provider(&self) -> impl neo_blockchain::LedgerProvider + '_ {
        LOCAL_LEDGER_PROVIDER_FACTORY.provider(self.snapshot.as_ref())
    }
}

impl<B: neo_storage::CacheRead> neo_network::BlockSource for LedgerBlockSource<B> {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        self.persisted_provider()
            .block_by_index(index)
            .ok()
            .flatten()
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        self.persisted_provider()
            .header_by_index(index)
            .ok()
            .flatten()
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        self.persisted_provider()
            .block_hash_by_index(index)
            .ok()
            .flatten()
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        self.persisted_provider().block_by_hash(hash).ok().flatten()
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        self.persisted_provider()
            .block_index_by_hash(hash)
            .ok()
            .flatten()
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        // Serve unconfirmed transactions from the mempool first (C# `GetData`
        // serves `MemoryPool` entries), then fall back to the ledger.
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        self.persisted_provider().transaction_by_hash(hash).ok()?
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        self.mempool.contains(hash)
            || self
                .persisted_provider()
                .contains_transaction(hash)
                .unwrap_or(false)
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        self.mempool
            .verified_snapshot()
            .iter()
            .map(|item| item.hash())
            .collect()
    }
}

#[cfg(test)]
#[path = "../../tests/node/ledger_source/local.rs"]
mod tests;

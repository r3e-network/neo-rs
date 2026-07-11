//! Store-backed block source for local ledger mode.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::node::recovery::LocalReplayGuard;
use neo_blockchain::{
    BlockProvider, HotColdLedgerProviderFactory, LedgerProviderFactory, OptionalLedgerProvider,
    StaticLedgerProvider, TxProvider,
};
use tracing::error;

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
    provider_factory: HotColdLedgerProviderFactory<OptionalLedgerProvider<StaticLedgerProvider>>,
    replay_guard: Arc<LocalReplayGuard>,
    provider_failed: AtomicBool,
}

impl<B: neo_storage::CacheRead> LedgerBlockSource<B> {
    pub(in crate::node) fn new(
        snapshot: Arc<neo_storage::persistence::DataCache<B>>,
        ledger: Arc<neo_blockchain::LedgerContext>,
        mempool: Arc<neo_mempool::MemoryPool>,
        static_archive: Option<neo_blockchain::StaticLedgerArchive>,
        replay_guard: Arc<LocalReplayGuard>,
    ) -> Self {
        Self {
            snapshot,
            ledger,
            mempool,
            provider_factory: HotColdLedgerProviderFactory::new(
                OptionalLedgerProvider::from_option(
                    static_archive.map(|archive| archive.provider()),
                ),
            ),
            replay_guard,
            provider_failed: AtomicBool::new(false),
        }
    }

    /// Creates the canonical durable ledger provider for this snapshot.
    fn persisted_provider(&self) -> impl neo_blockchain::LedgerProvider + '_ {
        self.provider_factory.provider(self.snapshot.as_ref())
    }

    fn report_provider_failure(
        &self,
        operation: &'static str,
        detail: impl std::fmt::Display,
        error_value: &impl std::fmt::Display,
    ) {
        if self.provider_failed.swap(true, Ordering::AcqRel) {
            return;
        }

        error!(
            target: "neo::ledger_source",
            operation,
            detail = %detail,
            error = %error_value,
            "local ledger provider read failed; requesting restart"
        );
        self.replay_guard
            .request_recoverable_restart("local ledger provider read failed while serving peers");
    }

    #[inline]
    fn provider_is_healthy(&self) -> bool {
        !self.provider_failed.load(Ordering::Acquire)
    }
}

impl<B: neo_storage::CacheRead> neo_network::BlockSource for LedgerBlockSource<B> {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        if !self.provider_is_healthy() {
            return None;
        }
        match self.persisted_provider().block_by_index(index) {
            Ok(block) => block,
            Err(error) => {
                self.report_provider_failure("block_by_index", index, &error);
                None
            }
        }
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        if !self.provider_is_healthy() {
            return None;
        }
        match self.persisted_provider().header_by_index(index) {
            Ok(header) => header,
            Err(error) => {
                self.report_provider_failure("header_by_index", index, &error);
                None
            }
        }
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        if !self.provider_is_healthy() {
            return None;
        }
        match self.persisted_provider().block_hash_by_index(index) {
            Ok(hash) => hash,
            Err(error) => {
                self.report_provider_failure("block_hash_by_index", index, &error);
                None
            }
        }
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        if !self.provider_is_healthy() {
            return None;
        }
        match self.persisted_provider().block_by_hash(hash) {
            Ok(block) => block,
            Err(error) => {
                self.report_provider_failure("block_by_hash", hash, &error);
                None
            }
        }
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        if !self.provider_is_healthy() {
            return None;
        }
        match self.persisted_provider().block_index_by_hash(hash) {
            Ok(index) => index,
            Err(error) => {
                self.report_provider_failure("block_index_by_hash", hash, &error);
                None
            }
        }
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        if !self.provider_is_healthy() {
            return None;
        }
        // Serve unconfirmed transactions from the mempool first (C# `GetData`
        // serves `MemoryPool` entries), then fall back to the ledger.
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        match self.persisted_provider().transaction_by_hash(hash) {
            Ok(transaction) => transaction,
            Err(error) => {
                self.report_provider_failure("transaction_by_hash", hash, &error);
                None
            }
        }
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        self.provider_is_healthy()
            .then(|| self.ledger.get_extensible(hash))
            .flatten()
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        self.provider_is_healthy()
            && (self.mempool.contains(hash)
                || match self.persisted_provider().contains_transaction(hash) {
                    Ok(contains) => contains,
                    Err(error) => {
                        self.report_provider_failure("contains_transaction", hash, &error);
                        false
                    }
                })
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        if !self.provider_is_healthy() {
            return Vec::new();
        }
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

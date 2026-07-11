//! Cold Ledger fallback behavior at the blockchain command boundary.

use std::sync::Arc;

use neo_error::CoreResult;
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256, VerifyResult};

use super::*;
use crate::ledger_provider::{
    BlockProvider, HotColdLedgerProvider, LedgerProvider, StorageLedgerProvider,
    TransactionStateProvider, TxProvider,
};

#[derive(Debug, Default)]
struct TestMempool;

impl MempoolLike for TestMempool {
    fn try_add<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn try_add_cached<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
        _cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }
}

#[derive(Clone, Debug)]
struct ColdBlockProvider {
    block: Block,
}

impl BlockProvider for ColdBlockProvider {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        Ok((index == self.block.index()).then(|| self.block.hash()))
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok((*hash == self.block.hash()).then(|| self.block.header.clone()))
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        Ok((*hash == self.block.hash()).then(|| self.block.clone()))
    }
}

impl TxProvider for ColdBlockProvider {
    fn transaction_by_hash(&self, _hash: &UInt256) -> CoreResult<Option<Transaction>> {
        Ok(None)
    }
}

impl TransactionStateProvider for ColdBlockProvider {
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

struct ColdReadContext {
    snapshot: Arc<neo_storage::DataCache>,
    cold: ColdBlockProvider,
}

impl std::fmt::Debug for ColdReadContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ColdReadContext")
            .field("cold_height", &self.cold.block.index())
            .finish_non_exhaustive()
    }
}

impl crate::service_context::SystemContext for ColdReadContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::new(neo_config::ProtocolSettings::default())
    }

    fn current_height(&self) -> u32 {
        self.cold.block.index()
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn ledger_provider<'a>(
        &'a self,
        snapshot: &'a neo_storage::DataCache,
    ) -> impl LedgerProvider + crate::ChainTipProvider + 'a {
        HotColdLedgerProvider::new(StorageLedgerProvider::new(snapshot), self.cold.clone())
    }
}

#[tokio::test]
async fn command_loop_uses_system_context_cold_provider_after_hot_cache_eviction() {
    let mut header = Header::new();
    header.set_index(7);
    let block = Block::from_parts(header, Vec::new());
    let hash = block.hash();
    let system = Arc::new(ColdReadContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        cold: ColdBlockProvider {
            block: block.clone(),
        },
    });
    let (service, handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    let task = tokio::spawn(service.run());

    let loaded = handle
        .get_block(&hash)
        .await
        .expect("get archived block")
        .expect("cold block");
    assert_eq!(loaded.hash(), block.hash());

    drop(handle);
    task.await.expect("service task");
}

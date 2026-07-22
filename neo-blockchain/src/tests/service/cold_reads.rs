//! Cold Ledger fallback behavior at the blockchain command boundary.

use std::sync::Arc;

use neo_config::{ChainSpecProvider, NeoChainSpec};
use neo_error::CoreResult;
use neo_mempool::{TransactionAdmissionError, TransactionAdmissionOutcome, TransactionOrigin};
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256};

use super::*;
use crate::ledger_provider::{
    BlockProvider, HotColdLedgerProvider, LedgerProvider, StorageLedgerProvider,
    TransactionStateProvider, TxProvider,
};

#[derive(Debug, Default)]
struct TestMempool;

impl MempoolLike for TestMempool {
    fn add_transaction<B, L>(
        &self,
        origin: TransactionOrigin,
        transaction: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _ledger_provider: &L,
    ) -> TransactionAdmissionOutcome
    where
        B: neo_storage::CacheRead,
        L: neo_mempool::AdmissionLedgerProvider,
    {
        match transaction.try_hash() {
            Ok(hash) => TransactionAdmissionOutcome::Accepted { hash, origin },
            Err(error) => TransactionAdmissionOutcome::Error {
                hash: None,
                origin,
                error: TransactionAdmissionError::InvalidHash(error.to_string()),
            },
        }
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
    chain_spec: Arc<NeoChainSpec>,
    snapshot: Arc<neo_storage::DataCache>,
    cold: ColdBlockProvider,
}

impl ChainSpecProvider for ColdReadContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
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
        chain_spec: neo_test_fixtures::test_chain_spec(neo_config::ProtocolSettings::default()),
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

//! Composed runtime node.
//!
//! The [`Node`] struct is the top-level runtime container — what
//! the `neo-node` binary actually constructs at startup. It owns:
//!
//! - the typed blockchain handle from `neo-blockchain`,
//! - the typed network handle from `neo-network`,
//! - the typed staged-sync pipeline used by downloader composition,
//! - the live peer-import adapter sharing that pipeline's bounded queue,
//! - a [`WalletProvider`] for the optional node wallet,
//! - the storage backend, mempool, and header cache,
//! - the native contract provider owned for NeoVM host calls,
//! - and the node-wide hot/cold Ledger provider factory.
//!
//! Construction goes through [`crate::NodeCoreBuilder`] and the final
//! [`crate::NodeBuilder`]. Every required capability is a constructor argument,
//! so a partially configured node graph cannot be represented. There are no
//! trait-object executor, consensus, or engine fields to compose; those were
//! removed in ADR-032 / ADR-033.

use std::sync::Arc;
use std::time::Duration;

use neo_blockchain::ledger_provider::TransactionAdmissionLedger;
use neo_blockchain::{
    BlockchainHandle, HeaderCache, HotColdLedgerProviderFactory, LedgerProviderFactory,
    OptionalStaticLedgerProvider,
};
use neo_config::{ChainSpecProvider, NeoChainSpec};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::{MemoryPool, TransactionAdmissionOutcome, TransactionOrigin, TxPoolConfig};
use neo_network::NetworkHandle;
use neo_payloads::Transaction;
use neo_runtime::{
    SharedStoreSyncStageCheckpointStore, SharedStoreVerifiedHeaderStore, StoreProvider, TxAdmission,
};
use neo_storage::persistence::TransactionalStore;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::store_cache::StoreCache;

use crate::live_block_import_pipeline::LiveBlockImportPipeline;
use crate::staged_sync_pipeline::StagedSyncPipeline;
use crate::wallet_provider::WalletProvider;

/// The composed Neo node runtime.
///
/// Cheap to clone — every field is either `Clone` (handles) or
/// `Arc<T>` (shared state).
#[derive(Clone)]
pub struct Node<P = neo_native_contracts::StandardNativeProvider, S = MemoryStore>
where
    P: NativeContractProvider,
    S: TransactionalStore,
{
    /// Immutable chain specification selected at startup.
    pub(super) chain_spec: Arc<NeoChainSpec>,

    /// Shared storage backend.
    ///
    /// The node keeps the concrete `S: TransactionalStore` type throughout composition.
    /// Runtime-selected startup uses `RuntimeStore`, a concrete enum over the
    /// supported backends.
    pub(super) storage: Arc<S>,

    /// Wallet provider (current wallet, if any).
    pub(super) wallets: WalletProvider,

    /// Blockchain service handle. The [`Node`] clones this and hands
    /// it to RPC handlers, consensus, plugins, etc.
    pub(super) blockchain: BlockchainHandle,

    /// Network service handle. Other subsystems call methods on
    /// this to broadcast blocks / transactions.
    pub(super) network: NetworkHandle,

    /// Shared `Headers -> Bodies -> Import` pipeline entry point.
    ///
    /// The handle owns durable verified-header staging, the body/header gate,
    /// bounded preverification, and stage checkpoints over the same
    /// blockchain/cache/storage graph as the rest of the node.
    pub(super) staged_sync_pipeline: Arc<
        StagedSyncPipeline<
            SharedStoreSyncStageCheckpointStore<S>,
            SharedStoreVerifiedHeaderStore<S>,
        >,
    >,

    /// Live peer-inventory preflight sharing the staged-sync import queue.
    pub(super) live_block_import_pipeline: Arc<LiveBlockImportPipeline>,

    /// Shared memory pool. Blockchain, RPC, P2P, and node-generated
    /// transactions all enter this instance through its typed admission
    /// operation; RPC reads the same pool for `getrawmempool` and conflict
    /// projections.
    pub(super) mempool: Arc<MemoryPool<P>>,

    /// Shared header cache: headers that are ahead of the persisted
    /// tip. The node binary hands the same instance to the blockchain
    /// service so RPC `getblockheadercount` sees the live cache.
    pub(super) header_cache: Arc<HeaderCache>,

    /// Native-contract provider captured for NeoVM host calls.
    ///
    /// Stored on the node so the composition-root dependency remains visible
    /// after `NodeBuilder::build()` instead of disappearing into the global
    /// `neo-execution` lookup seam.
    pub(super) native_contract_provider: Arc<P>,

    /// Shared hot/cold Ledger read policy selected by the composition root.
    pub(super) ledger_provider_factory: HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>,
}

impl<P, S> std::fmt::Debug for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("chain_spec", &self.chain_spec.identity())
            .field("storage", &"<Store>")
            .field("wallets", &self.wallets)
            .field("blockchain", &"BlockchainHandle")
            .field("network", &"NetworkHandle")
            .field("staged_sync_pipeline", &self.staged_sync_pipeline)
            .field(
                "live_block_import_pipeline",
                &self.live_block_import_pipeline,
            )
            .field("mempool", &self.mempool.total_count())
            .field("header_cache", &self.header_cache.count())
            .field(
                "native_contract_provider_contracts",
                &self.native_contract_provider.all_native_contracts().len(),
            )
            .field(
                "cold_ledger_provider",
                &self.ledger_provider_factory.cold().is_enabled(),
            )
            .finish()
    }
}

impl<P, S> Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    /// Returns the immutable chain specification selected at startup.
    pub fn chain_spec(&self) -> Arc<NeoChainSpec> {
        Arc::clone(&self.chain_spec)
    }

    /// Returns the blockchain service handle.
    pub fn blockchain(&self) -> BlockchainHandle {
        self.blockchain.clone()
    }

    /// Returns the network service handle.
    pub fn network(&self) -> NetworkHandle {
        self.network.clone()
    }

    /// Returns the composed `Headers -> Bodies -> Import` pipeline handle.
    pub fn staged_sync_pipeline(
        &self,
    ) -> Arc<
        StagedSyncPipeline<
            SharedStoreSyncStageCheckpointStore<S>,
            SharedStoreVerifiedHeaderStore<S>,
        >,
    > {
        Arc::clone(&self.staged_sync_pipeline)
    }

    /// Returns the live peer block-import adapter.
    pub fn live_block_import_pipeline(&self) -> Arc<LiveBlockImportPipeline> {
        Arc::clone(&self.live_block_import_pipeline)
    }

    /// Returns the storage backend.
    pub fn storage(&self) -> Arc<S> {
        Arc::clone(&self.storage)
    }

    /// Returns a fresh [`StoreCache`] over the node's storage backend.
    ///
    /// This is the replacement for the legacy `NeoSystem::store_cache`:
    /// a write-through cache view whose `commit()` persists tracked
    /// changes back into the shared store. Each call returns an
    /// independent cache over the *same* underlying store, so reads
    /// observe everything previously committed through any other view.
    pub fn store_cache(&self) -> StoreCache<S> {
        StoreCache::<S>::new_from_store(Arc::clone(&self.storage), false)
    }

    /// Returns the shared memory pool.
    pub fn mempool(&self) -> Arc<MemoryPool<P>> {
        Arc::clone(&self.mempool)
    }

    /// Returns the shared header cache (headers ahead of the persisted
    /// tip).
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        Arc::clone(&self.header_cache)
    }

    /// Returns the native-contract provider shared by execution-facing
    /// services composed into this node.
    pub fn native_contract_provider(&self) -> Arc<P> {
        Arc::clone(&self.native_contract_provider)
    }

    /// Returns the node-wide routed Ledger provider factory.
    #[must_use]
    pub const fn ledger_provider_factory(
        &self,
    ) -> &HotColdLedgerProviderFactory<OptionalStaticLedgerProvider> {
        &self.ledger_provider_factory
    }

    /// Returns the configured immutable Ledger fallback.
    #[must_use]
    pub fn cold_ledger_provider(&self) -> OptionalStaticLedgerProvider {
        self.ledger_provider_factory.cold().clone()
    }

    /// Maximum increment of `valid_until_block` over the current
    /// height, from the protocol settings (C#
    /// `ProtocolSettings.MaxValidUntilBlockIncrement`).
    pub fn max_valid_until_block_increment(&self) -> u32 {
        self.chain_spec
            .protocol_settings()
            .max_valid_until_block_increment
    }

    /// Target time between blocks, from the protocol settings (C#
    /// `ProtocolSettings.MillisecondsPerBlock`).
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(u64::from(
            self.chain_spec.protocol_settings().milliseconds_per_block,
        ))
    }

    /// Maximum number of traceable blocks, from the protocol settings
    /// (C# `ProtocolSettings.MaxTraceableBlocks`).
    pub fn max_traceable_blocks(&self) -> u32 {
        self.chain_spec.protocol_settings().max_traceable_blocks
    }

    /// Returns the wallet provider.
    pub fn wallets(&self) -> WalletProvider {
        self.wallets.clone()
    }

    /// Submit a transaction through the node's canonical mempool boundary and
    /// apply the propagation policy carried by its origin.
    pub fn submit_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Transaction,
    ) -> TransactionAdmissionOutcome {
        let store_cache = StoreCache::<S>::new_from_snapshot(self.storage.snapshot());
        let snapshot = store_cache.data_cache();
        let ledger =
            TransactionAdmissionLedger::new(self.ledger_provider_factory.provider(snapshot));
        let relay = origin.should_propagate().then(|| transaction.clone());
        let outcome = self
            .mempool
            .add_transaction(origin, transaction, snapshot, &ledger);
        if outcome.is_accepted() {
            if let Some(transaction) = relay {
                let _ = self.network.try_broadcast_transaction(transaction);
            }
        }
        outcome
    }
}

impl Node<neo_native_contracts::StandardNativeProvider, MemoryStore> {
    /// Construct an in-memory node for test-only RPC/service fixtures.
    pub fn for_test(chain_spec: std::sync::Arc<NeoChainSpec>) -> Self {
        let storage = Arc::new(MemoryStore::new());
        let native_contract_provider =
            Arc::new(neo_native_contracts::StandardNativeProvider::new());
        let mempool = Arc::new(MemoryPool::new_with_native_contract_provider(
            Arc::clone(&chain_spec),
            TxPoolConfig::default(),
            Arc::clone(&native_contract_provider),
        ));
        let (blockchain, _rx) = neo_blockchain::BlockchainHandle::with_capacity();
        let (network, _nrx, _etx) = neo_network::NetworkHandle::channel(8, 8);
        crate::NodeBuilder::new(
            chain_spec,
            storage,
            blockchain,
            network,
            mempool,
            Arc::new(HeaderCache::default()),
            native_contract_provider,
        )
        .build()
    }
}

impl<P, S> StoreProvider for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    type Store = S;

    fn store(&self) -> Arc<S> {
        Arc::clone(&self.storage)
    }

    fn store_cache(&self) -> StoreCache<S> {
        StoreCache::<S>::new_from_store(Arc::clone(&self.storage), false)
    }
}

impl<P, S> ChainSpecProvider for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl<P, S> TxAdmission for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    type Origin = TransactionOrigin;
    type Outcome = TransactionAdmissionOutcome;

    fn submit_transaction(
        &self,
        origin: Self::Origin,
        transaction: neo_payloads::Transaction,
    ) -> Self::Outcome {
        Node::submit_transaction(self, origin, transaction)
    }
}

#[cfg(test)]
#[path = "../tests/composition/node.rs"]
mod tests;

//! Node context for the RPC server.
//!
//! [`NodeContext`] is the service handle bundle that [`RpcServer`] stores
//! instead of `Arc<neo_system::Node>`. It holds the same concrete handles
//! (blockchain, network, mempool, storage, chain specification, …) but is defined in
//! `neo-rpc` itself, so the RPC crate no longer needs to depend on the
//! composition root (`neo-system`).
//!
//! The composition root (typically `neo-node` or `neo-system`) constructs
//! a `NodeContext` from its `Node` and passes it to `RpcServer::new()`.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. It must not import `neo_system::Node`
//! in production code. The `From<Node>` conversion lives in `neo-system`
//! (or the binary) so the dependency direction stays downward.

use std::sync::Arc;
use std::time::Duration;

use neo_blockchain::ledger_provider::TransactionAdmissionLedger;
use neo_blockchain::{
    BlockchainHandle, HeaderCache, HotColdLedgerProvider, HotColdLedgerProviderFactory,
    LedgerProviderFactory, OptionalStaticLedgerProvider, StorageLedgerProvider,
};
use neo_config::{ChainSpecProvider, NeoChainSpec, ProtocolSettings};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::{MemoryPool, TransactionAdmissionOutcome, TransactionOrigin};
use neo_network::NetworkHandle;
use neo_payloads::Transaction;
use neo_runtime::{StoreProvider, TxAdmission};
use neo_storage::persistence::DataCache;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_cache::StoreCache;

use super::rpc_services::RpcServices;

/// Service handle bundle for the RPC server.
///
/// Replaces `Arc<neo_system::Node>` in `neo-rpc`. Constructed by the
/// composition root and passed to `RpcServer::new()`.
///
/// Cheap to clone — every field is either `Clone` (handles) or `Arc<T>`.
#[derive(Clone)]
pub struct NodeContext<P = neo_native_contracts::StandardNativeProvider, S = RuntimeStore>
where
    P: NativeContractProvider,
    S: Store,
{
    /// Immutable chain specification selected by the composition root.
    chain_spec: Arc<NeoChainSpec>,

    /// Storage backend.
    storage: Arc<S>,

    /// Blockchain service handle.
    blockchain: BlockchainHandle,

    /// Network service handle.
    network: NetworkHandle,

    /// Shared memory pool.
    mempool: Arc<MemoryPool<P>>,

    /// Shared header cache.
    header_cache: Arc<HeaderCache>,

    /// Named, statically typed optional services used by RPC handlers.
    services: RpcServices<S>,

    /// Native-contract provider for NeoVM host calls.
    native_contract_provider: Arc<P>,

    /// Node-wide routed Ledger read policy, including the configured immutable
    /// fallback when static files are enabled.
    ledger_provider_factory: HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>,
}

impl<P, S> std::fmt::Debug for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeContext")
            .field("chain_spec", &self.chain_spec.identity())
            .field("storage", &"<Store>")
            .field("blockchain", &"BlockchainHandle")
            .field("network", &"NetworkHandle")
            .field("mempool", &self.mempool.total_count())
            .field("header_cache", &self.header_cache.count())
            .field("services", &self.services)
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

impl<P, S> NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    /// Construct a `NodeContext` from its component parts.
    ///
    /// The application composition root passes named capabilities obtained
    /// from the core node. `NodeContext` deliberately does not depend on the
    /// concrete `neo_system::Node` layout.
    pub fn from_parts(
        chain_spec: Arc<NeoChainSpec>,
        storage: Arc<S>,
        blockchain: BlockchainHandle,
        network: NetworkHandle,
        mempool: Arc<MemoryPool<P>>,
        header_cache: Arc<HeaderCache>,
        services: RpcServices<S>,
        native_contract_provider: Arc<P>,
        cold_ledger_provider: OptionalStaticLedgerProvider,
    ) -> Self {
        Self {
            chain_spec,
            storage,
            blockchain,
            network,
            mempool,
            header_cache,
            services,
            native_contract_provider,
            ledger_provider_factory: HotColdLedgerProviderFactory::new(cold_ledger_provider),
        }
    }

    /// Returns the protocol settings.
    pub fn settings(&self) -> Arc<ProtocolSettings> {
        self.chain_spec.protocol_settings_arc()
    }

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

    /// Returns the storage backend.
    pub fn storage(&self) -> Arc<S> {
        Arc::clone(&self.storage)
    }

    /// Returns a fresh [`StoreCache`] over the node's storage backend.
    pub fn store_cache(&self) -> StoreCache<S> {
        StoreCache::<S>::new_from_store(Arc::clone(&self.storage), false)
    }

    /// Returns the shared memory pool.
    pub fn mempool(&self) -> Arc<MemoryPool<P>> {
        Arc::clone(&self.mempool)
    }

    /// Submit through the canonical mempool boundary and apply origin-based
    /// propagation only after admission succeeds.
    pub fn submit_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Transaction,
    ) -> TransactionAdmissionOutcome {
        let store_cache = StoreCache::<S>::new_from_snapshot(self.storage.snapshot());
        let snapshot = store_cache.data_cache();
        let provider = TransactionAdmissionLedger::new(self.ledger_provider(snapshot));
        let relay = origin.should_propagate().then(|| transaction.clone());
        let outcome = self
            .mempool
            .add_transaction(origin, transaction, snapshot, &provider);
        if outcome.is_accepted() {
            if let Some(transaction) = relay {
                let _ = self.network.try_broadcast_transaction(transaction);
            }
        }
        outcome
    }

    /// Returns the shared header cache.
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        Arc::clone(&self.header_cache)
    }

    /// Maximum increment of `valid_until_block` over the current height.
    pub fn max_valid_until_block_increment(&self) -> u32 {
        self.chain_spec
            .protocol_settings()
            .max_valid_until_block_increment
    }

    /// Target time between blocks.
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(u64::from(
            self.chain_spec.protocol_settings().milliseconds_per_block,
        ))
    }

    /// Maximum number of traceable blocks.
    pub fn max_traceable_blocks(&self) -> u32 {
        self.chain_spec.protocol_settings().max_traceable_blocks
    }

    /// Returns the typed RPC service bundle.
    pub fn services(&self) -> &RpcServices<S> {
        &self.services
    }

    /// Returns the state-service store, if one was started.
    pub fn state_store(&self) -> Option<Arc<neo_state_service::StateStore<S>>> {
        self.services.state_store()
    }

    /// Returns the indexer service, if one was started.
    pub fn indexer_service(&self) -> Option<Arc<neo_indexer::IndexerService>> {
        self.services.indexer()
    }

    /// Returns the application-log service, if one was started.
    pub fn application_logs_service(
        &self,
    ) -> Option<Arc<crate::application_logs::ApplicationLogsService<S>>> {
        self.services.application_logs()
    }

    /// Returns the token-tracker service, if one was started.
    pub fn tokens_tracker_service(
        &self,
    ) -> Option<Arc<crate::plugins::tokens_tracker::TokensTrackerService<S>>> {
        self.services.tokens_tracker()
    }

    /// Returns the native-contract provider.
    pub fn native_contract_provider(&self) -> Arc<P> {
        Arc::clone(&self.native_contract_provider)
    }

    /// Creates the shared hot/cold Ledger provider over `snapshot`.
    pub fn ledger_provider<'a, B: neo_storage::CacheRead>(
        &'a self,
        snapshot: &'a DataCache<B>,
    ) -> impl neo_blockchain::LedgerProvider + neo_blockchain::ChainTipProvider + 'a {
        self.ledger_provider_factory.provider(snapshot)
    }
}

impl<P, S> LedgerProviderFactory for NodeContext<P, S>
where
    P: NativeContractProvider,
    S: Store,
{
    type Provider<'a, B>
        = HotColdLedgerProvider<StorageLedgerProvider<'a, B>, OptionalStaticLedgerProvider>
    where
        Self: 'a,
        B: neo_storage::CacheRead;

    fn provider<'a, B: neo_storage::CacheRead>(
        &'a self,
        snapshot: &'a DataCache<B>,
    ) -> Self::Provider<'a, B> {
        self.ledger_provider_factory.provider(snapshot)
    }
}

// =============================================================================
// Provider trait implementations
//
// These allow `NodeContext` to be used anywhere the provider traits are
// expected while preserving the concrete node context type at call sites that
// are generic over providers.
// =============================================================================

impl<P, S> StoreProvider for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    type Store = S;

    fn store(&self) -> Arc<S> {
        Arc::clone(&self.storage)
    }

    fn store_cache(&self) -> StoreCache<S> {
        StoreCache::<S>::new_from_store(Arc::clone(&self.storage), false)
    }
}

impl<P, S> ChainSpecProvider for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl<P, S> TxAdmission for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    type Origin = TransactionOrigin;
    type Outcome = TransactionAdmissionOutcome;

    fn submit_transaction(&self, origin: Self::Origin, transaction: Transaction) -> Self::Outcome {
        NodeContext::submit_transaction(self, origin, transaction)
    }
}

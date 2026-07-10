//! Node context for the RPC server.
//!
//! [`NodeContext`] is the service handle bundle that [`RpcServer`] stores
//! instead of `Arc<neo_system::Node>`. It holds the same concrete handles
//! (blockchain, network, mempool, storage, settings, …) but is defined in
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

use neo_blockchain::{
    BlockchainHandle, EmptyLedgerProvider, HeaderCache, HotColdLedgerProviderFactory,
    LedgerProviderFactory, TransactionStateProvider, TxProvider,
};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_payloads::{Transaction, VerifyResult};
use neo_runtime::{ConfigProvider, ServiceError, StoreProvider, TxAdmission};
use neo_storage::persistence::DataCache;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_cache::StoreCache;

use super::native_provider::NativeProviderAdapter;
use super::rpc_services::RpcServices;

const NODE_CONTEXT_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

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
    /// Protocol settings the node is running with.
    settings: Arc<ProtocolSettings>,

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
}

impl<P, S> std::fmt::Debug for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeContext")
            .field("settings", &"<ProtocolSettings>")
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
        settings: Arc<ProtocolSettings>,
        storage: Arc<S>,
        blockchain: BlockchainHandle,
        network: NetworkHandle,
        mempool: Arc<MemoryPool<P>>,
        header_cache: Arc<HeaderCache>,
        services: RpcServices<S>,
        native_contract_provider: Arc<P>,
    ) -> Self {
        Self {
            settings,
            storage,
            blockchain,
            network,
            mempool,
            header_cache,
            services,
            native_contract_provider,
        }
    }

    /// Returns the protocol settings.
    pub fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
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

    /// Returns the shared header cache.
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        Arc::clone(&self.header_cache)
    }

    /// Maximum increment of `valid_until_block` over the current height.
    pub fn max_valid_until_block_increment(&self) -> u32 {
        self.settings.max_valid_until_block_increment
    }

    /// Target time between blocks.
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(u64::from(self.settings.milliseconds_per_block))
    }

    /// Maximum number of traceable blocks.
    pub fn max_traceable_blocks(&self) -> u32 {
        self.settings.max_traceable_blocks
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

impl<P, S> ConfigProvider for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn max_valid_until_block_increment(&self) -> u32 {
        self.settings.max_valid_until_block_increment
    }
}

impl<P, S> TxAdmission for NodeContext<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn try_enqueue_preverify<B: neo_storage::CacheRead>(
        &self,
        tx: Transaction,
        relay: bool,
        snapshot: &DataCache<B>,
    ) -> Result<(), ServiceError> {
        let hash = tx
            .try_hash()
            .map_err(|_| ServiceError::internal(format!("{:?}", VerifyResult::Invalid)))?;
        let ledger = NODE_CONTEXT_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        if ledger.contains_transaction(&hash).map_err(|error| {
            ServiceError::internal(format!("ledger contains_transaction: {error}"))
        })? {
            return Err(ServiceError::internal(format!(
                "{:?}",
                VerifyResult::AlreadyExists
            )));
        }

        let native = NativeProviderAdapter::new(Arc::clone(&self.native_contract_provider));
        let max_traceable_blocks = native
            .max_traceable_blocks(snapshot, self.settings.as_ref())
            .map_err(|error| ServiceError::internal(format!("MaxTraceableBlocks: {error}")))?;
        let signers: Vec<_> = tx.signers().iter().map(|signer| signer.account).collect();
        if ledger
            .contains_conflict_hash(&hash, &signers, max_traceable_blocks)
            .map_err(|error| {
                ServiceError::internal(format!("ledger contains_conflict_hash: {error}"))
            })?
        {
            return Err(ServiceError::internal(format!(
                "{:?}",
                VerifyResult::HasConflicts
            )));
        }

        let result = self.mempool.try_add(tx.clone(), snapshot);
        if result != VerifyResult::Succeed {
            return Err(ServiceError::internal(format!("{result:?}")));
        }
        if relay {
            let _ = self.network.try_broadcast_transaction(tx);
        }
        Ok(())
    }
}

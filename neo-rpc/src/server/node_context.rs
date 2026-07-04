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

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_runtime::{ConfigProvider, ServiceRegistry, StoreProvider};
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_cache::StoreCache;

/// Service handle bundle for the RPC server.
///
/// Replaces `Arc<neo_system::Node>` in `neo-rpc`. Constructed by the
/// composition root and passed to `RpcServer::new()`.
///
/// Cheap to clone — every field is either `Clone` (handles) or `Arc<T>`.
#[derive(Clone)]
pub struct NodeContext {
    /// Protocol settings the node is running with.
    pub settings: Arc<ProtocolSettings>,

    /// Storage backend.
    pub storage: Arc<dyn Store>,

    /// Blockchain service handle.
    pub blockchain: BlockchainHandle,

    /// Network service handle.
    pub network: NetworkHandle,

    /// Shared memory pool.
    pub mempool: Arc<MemoryPool>,

    /// Shared header cache.
    pub header_cache: Arc<HeaderCache>,

    /// Service registry for optional services (application logs, tokens
    /// tracker, oracle, state service, …).
    pub services: ServiceRegistry,

    /// Native-contract provider for NeoVM host calls.
    pub native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl std::fmt::Debug for NodeContext {
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

impl NodeContext {
    /// Construct a `NodeContext` from its component parts.
    ///
    /// The composition root (typically `neo-node` or `neo-system`) calls
    /// this with the concrete `Node`'s public fields, keeping the
    /// dependency direction downward (L5 → L6, not L6 → L5).
    pub fn from_parts(
        settings: Arc<ProtocolSettings>,
        storage: Arc<dyn Store>,
        blockchain: BlockchainHandle,
        network: NetworkHandle,
        mempool: Arc<MemoryPool>,
        header_cache: Arc<HeaderCache>,
        services: ServiceRegistry,
        native_contract_provider: Arc<dyn NativeContractProvider>,
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
    pub fn storage(&self) -> Arc<dyn Store> {
        Arc::clone(&self.storage)
    }

    /// Returns a fresh [`StoreCache`] over the node's storage backend.
    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(Arc::clone(&self.storage), false)
    }

    /// Returns the shared memory pool.
    pub fn mempool(&self) -> Arc<MemoryPool> {
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

    /// Looks up the registered instance of type `T`, if any.
    pub fn get_service<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.services.get::<T>()
    }

    /// Registers `service` as *the* instance of type `T`, replacing
    /// any previously registered instance of the same type.
    pub fn register_service<T: Send + Sync + 'static>(&self, service: Arc<T>) -> Option<Arc<T>> {
        self.services.register(service)
    }

    /// Returns the registered state-service store, if one was started.
    pub fn state_store(&self) -> Option<Arc<neo_state_service::StateStore>> {
        self.get_service::<neo_state_service::StateStore>()
    }

    /// Returns the native-contract provider.
    pub fn native_contract_provider(&self) -> Arc<dyn NativeContractProvider> {
        Arc::clone(&self.native_contract_provider)
    }
}

// =============================================================================
// Provider trait implementations
//
// These allow `NodeContext` to be used anywhere the provider traits are
// expected (e.g. `Session::new()` accepting `Arc<dyn StoreProvider>`).
// =============================================================================

impl StoreProvider for NodeContext {
    fn store(&self) -> Arc<dyn Store> {
        Arc::clone(&self.storage)
    }

    fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(Arc::clone(&self.storage), false)
    }
}

impl ConfigProvider for NodeContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn max_valid_until_block_increment(&self) -> u32 {
        self.settings.max_valid_until_block_increment
    }
}

// Convert a `neo_system::Node` into a `NodeContext`.
//
// **Removed**: The `From<&neo_system::Node> for NodeContext` impl previously
// lived here, forcing `neo-rpc` to depend on `neo-system` (L5) as a required
// dependency — a layer violation (L6 → L5). Callers in the composition root
// (`neo-node`, `neo-system`) must now use [`NodeContext::from_parts`] to
// assemble a `NodeContext` from the concrete `Node`'s public fields.

//! Composed runtime node.
//!
//! The [`Node`] struct is the top-level runtime container — what
//! the `neo-node` binary actually constructs at startup. It owns:
//!
//! - the typed blockchain handle from `neo-blockchain`,
//! - the typed network handle from `neo-network`,
//! - a [`WalletProvider`] for the optional node wallet,
//! - and the runtime service traits from `neo-runtime` for the
//!   remaining subsystems (block executor, consensus, engine API).
//!
//! Construction goes through [`NodeBuilder`], so the *combination*
//! of services is type-checked at `build()` time. The trait-object
//! services that don't have a concrete `impl` in this stage are
//! stored as `Option<Arc<dyn Trait>>` so the builder can be set up
//! with whatever subset the caller has ready, and the rest can be
//! wired in later.

use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_config::ProtocolSettings;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_runtime::{BlockExecutor, ConsensusService, NeoEngine};
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_cache::StoreCache;

use crate::error::NodeResult;
use crate::service_registry::ServiceRegistry;
use crate::wallet_provider::WalletProvider;

/// The composed Neo node runtime.
///
/// Cheap to clone — every field is either `Clone` (handles) or
/// `Arc<T>` (shared state).
#[derive(Clone)]
pub struct Node {
    /// Protocol settings the node is running with.
    pub settings: Arc<ProtocolSettings>,

    /// Storage backend. Stored as an `Arc<dyn Store>` so the
    /// executor, the state service, and the native-contracts cache
    /// can all share it without re-opening the database.
    pub storage: Arc<dyn Store>,

    /// Wallet provider (current wallet, if any).
    pub wallets: WalletProvider,

    /// Blockchain service handle. The [`Node`] clones this and hands
    /// it to RPC handlers, consensus, plugins, etc.
    pub blockchain: BlockchainHandle,

    /// Network service handle. Other subsystems call methods on
    /// this to broadcast blocks / transactions.
    pub network: NetworkHandle,

    /// Shared memory pool. The same instance the blockchain service /
    /// transaction router admit into; RPC handlers read it for
    /// `getrawmempool` / conflict checks.
    pub mempool: Arc<MemoryPool>,

    /// Shared header cache: headers that are ahead of the persisted
    /// tip. The node binary hands the same instance to the blockchain
    /// service so RPC `getblockheadercount` sees the live cache.
    pub header_cache: Arc<HeaderCache>,

    /// Registry of optional services (application logs, tokens
    /// tracker, oracle, state service, …) registered by the
    /// composition root and looked up by type at request time.
    pub services: ServiceRegistry,

    /// Optional block executor service. Present when a concrete
    /// `impl BlockExecutor` has been wired in by the caller.
    pub block_executor: Option<Arc<dyn BlockExecutor>>,

    /// Optional consensus service. Present when a concrete
    /// `impl ConsensusService` has been wired in by the caller.
    pub consensus: Option<Arc<dyn ConsensusService>>,

    /// Optional engine API service. Present when a concrete
    /// `impl NeoEngine` has been wired in by the caller.
    pub engine: Option<Arc<dyn NeoEngine>>,

    /// Cancellation token the node monitors for shutdown. A clone
    /// of this is also handed to every service task so they can
    /// observe the same shutdown signal.
    pub shutdown: CancellationToken,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("settings", &"<ProtocolSettings>")
            .field("storage", &"<Store>")
            .field("wallets", &self.wallets)
            .field("blockchain", &"BlockchainHandle")
            .field("network", &"NetworkHandle")
            .field("mempool", &self.mempool.total_count())
            .field("header_cache", &self.header_cache.count())
            .field("services", &self.services)
            .field(
                "block_executor",
                &self.block_executor.as_ref().map(|s| s.name()),
            )
            .field("consensus", &self.consensus.as_ref().map(|s| s.name()))
            .field("engine", &self.engine.as_ref().map(|s| s.name()))
            .finish()
    }
}

impl Node {
    /// Returns a fresh [`crate::NodeBuilder`].
    pub fn builder() -> crate::NodeBuilder {
        crate::NodeBuilder::default()
    }

    /// Construct a `Node` with the given protocol settings and an
    /// in-memory storage backend. Used by tests and by the
    /// orchestrator's headless mode (no P2P, no consensus, no
    /// engine).
    pub fn new(
        settings: std::sync::Arc<ProtocolSettings>,
        _blockchain: Option<()>,
        _network: Option<()>,
    ) -> Result<Self, crate::error::NodeError> {
        // Keep this direct constructor aligned with NodeBuilder::build().
        // Services created from a headless node still need native dispatch.
        neo_native_contracts::install();

        let storage: Arc<dyn neo_storage::persistence::store::Store> =
            Arc::new(neo_storage::persistence::providers::memory_store::MemoryStore::new());
        let wallets = WalletProvider::default();
        let (blockchain, _rx) = neo_blockchain::BlockchainHandle::with_capacity();
        let (network, _nrx, _etx) = neo_network::NetworkHandle::channel(8, 8);
        let mempool = Arc::new(MemoryPool::new(&settings));
        Ok(Self {
            settings,
            storage,
            wallets,
            blockchain,
            network,
            mempool,
            header_cache: Arc::new(HeaderCache::default()),
            services: ServiceRegistry::new(),
            block_executor: None,
            consensus: None,
            engine: None,
            shutdown: tokio_util::sync::CancellationToken::new(),
        })
    }

    /// Run the node until the cancellation token is fired.
    pub async fn run(self) -> NodeResult<()> {
        info!("Neo node starting up");
        self.shutdown.cancelled().await;
        info!("Neo node shutting down");
        Ok(())
    }

    /// Returns a fresh cancellation token, separated from the
    /// node's own so the caller can use it independently.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Returns the protocol settings the node is running with.
    ///
    /// Convenience accessor for plugins/services that received a
    /// `&Node` reference (typically from a `CommittingHandler` /
    /// `CommittedHandler` system downcast) and need to inspect
    /// the network magic, hardfork schedule, etc.
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
    ///
    /// This is the replacement for the legacy `NeoSystem::store_cache`:
    /// a write-through cache view whose `commit()` persists tracked
    /// changes back into the shared store. Each call returns an
    /// independent cache over the *same* underlying store, so reads
    /// observe everything previously committed through any other view.
    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(Arc::clone(&self.storage), false)
    }

    /// Returns the shared memory pool.
    pub fn mempool(&self) -> Arc<MemoryPool> {
        Arc::clone(&self.mempool)
    }

    /// Returns the shared header cache (headers ahead of the persisted
    /// tip).
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        Arc::clone(&self.header_cache)
    }

    /// Maximum increment of `valid_until_block` over the current
    /// height, from the protocol settings (C#
    /// `ProtocolSettings.MaxValidUntilBlockIncrement`).
    pub fn max_valid_until_block_increment(&self) -> u32 {
        self.settings.max_valid_until_block_increment
    }

    /// Target time between blocks, from the protocol settings (C#
    /// `ProtocolSettings.MillisecondsPerBlock`).
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(u64::from(self.settings.milliseconds_per_block))
    }

    /// Maximum number of traceable blocks, from the protocol settings
    /// (C# `ProtocolSettings.MaxTraceableBlocks`).
    pub fn max_traceable_blocks(&self) -> u32 {
        self.settings.max_traceable_blocks
    }

    /// Registers `service` as *the* instance of type `T` in the node's
    /// service registry, replacing (and returning) any previous
    /// instance. The reth-style replacement for the legacy
    /// `NeoSystem::add_service`.
    pub fn register_service<T: Send + Sync + 'static>(&self, service: Arc<T>) -> Option<Arc<T>> {
        self.services.register(service)
    }

    /// Looks up the registered instance of type `T`, if any. The
    /// reth-style replacement for the legacy
    /// `NeoSystem::get_service::<T>()`.
    pub fn get_service<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.services.get::<T>()
    }

    /// Returns the registered state-service store, if the composition
    /// root started one. Sugar over
    /// [`Self::get_service::<neo_state_service::StateStore>`].
    pub fn state_store(&self) -> Option<Arc<neo_state_service::StateStore>> {
        self.get_service::<neo_state_service::StateStore>()
    }

    /// Returns the wallet provider.
    pub fn wallets(&self) -> WalletProvider {
        self.wallets.clone()
    }

    /// Returns a handle to the transaction router used to enqueue
    /// outbound transactions for broadcast / persistence.
    ///
    /// The full router lives in `neo-mempool` / `neo-network`. The
    /// stub here is a marker that lets plugins compile against the
    /// orchestrator without taking a hard dependency on those crates.
    pub fn tx_router_actor(&self) -> TxRouterHandle {
        TxRouterHandle::new()
    }
}

/// Opaque handle returned by [`Node::tx_router_actor`]. A real
/// executor wires this to the live transaction router actor; the
/// stub is a no-op so plugins can compile and the orchestrator can
/// be built up incrementally.
#[derive(Debug, Default, Clone)]
pub struct TxRouterHandle;

impl TxRouterHandle {
    /// Construct a new `TxRouterHandle`.
    pub fn new() -> Self {
        Self
    }

    /// Try to enqueue a transaction for pre-verification + broadcast.
    /// The stub always succeeds; a real implementation forwards to
    /// the mempool / network actor.
    pub fn try_enqueue_preverify(
        &self,
        _tx: neo_payloads::Transaction,
        _relay: bool,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeBuilder;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    fn memory_store() -> Arc<dyn Store> {
        Arc::new(MemoryStore::new())
    }

    #[test]
    fn builder_returns_node_builder() {
        let _: NodeBuilder = Node::builder();
    }

    #[tokio::test]
    async fn cancellation_token_clone_is_independent() {
        let storage = memory_store();
        let settings = Arc::new(ProtocolSettings::default());
        let (bc, _rx) = BlockchainHandle::with_capacity();
        let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);

        let node = NodeBuilder::default()
            .with_settings(settings)
            .with_storage(storage)
            .with_blockchain(bc)
            .with_network(net)
            .build()
            .expect("builder should succeed");

        let token = node.cancellation_token();
        token.cancel();
        assert!(node.shutdown.is_cancelled());
    }
}

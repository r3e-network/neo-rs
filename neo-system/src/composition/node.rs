//! Composed runtime node.
//!
//! The [`Node`] struct is the top-level runtime container — what
//! the `neo-node` binary actually constructs at startup. It owns:
//!
//! - the typed blockchain handle from `neo-blockchain`,
//! - the typed network handle from `neo-network`,
//! - the typed sync import pipeline handle used by downloader composition,
//! - a [`WalletProvider`] for the optional node wallet,
//! - the storage backend, mempool, and header cache,
//! - and the native contract provider owned for NeoVM host calls.
//!
//! Construction goes through [`crate::NodeBuilder`], whose `build()`
//! validates the required components (storage, the blockchain and
//! network handles, the native provider) by null-checking each concrete
//! field and returning a descriptive missing-service error when one is
//! absent. There are no trait-object executor / consensus / engine
//! fields to compose: those were removed in ADR-032 / ADR-033.

use std::sync::Arc;
use std::time::Duration;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_payloads::{Transaction, VerifyResult};
use neo_runtime::{
    ConfigProvider, SharedStoreSyncStageCheckpointStore, StoreProvider, TxAdmission,
};
use neo_storage::DataCache;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_cache::StoreCache;

use super::tx_admission_provider::{
    NativeTxAdmissionLedgerProviderFactory, NativeTxAdmissionProvider, TxAdmissionLedgerProvider,
    TxAdmissionLedgerProviderFactory, TxAdmissionNativeProvider,
};
use crate::sync_import_pipeline::SyncImportPipeline;
use crate::wallet_provider::WalletProvider;
use neo_error::{CoreError, CoreResult};

/// The composed Neo node runtime.
///
/// Cheap to clone — every field is either `Clone` (handles) or
/// `Arc<T>` (shared state).
#[derive(Clone)]
pub struct Node<P = neo_native_contracts::StandardNativeProvider, S = MemoryStore>
where
    P: NativeContractProvider,
    S: Store,
{
    /// Protocol settings the node is running with.
    pub(super) settings: Arc<ProtocolSettings>,

    /// Shared storage backend.
    ///
    /// The node keeps the concrete `S: Store` type throughout composition.
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

    /// Shared sync import pipeline entry point.
    ///
    /// The handle owns the bounded preverification queue and durable
    /// import-stage checkpoint provider over the same blockchain/storage
    /// handles as the rest of the node. Live inventory still uses the
    /// inventory-aware blockchain handle directly until downloader
    /// integration is widened.
    pub(super) sync_import_pipeline:
        Arc<SyncImportPipeline<SharedStoreSyncStageCheckpointStore<S>>>,

    /// Shared memory pool. The same instance the blockchain service /
    /// transaction router admit into; RPC handlers read it for
    /// `getrawmempool` / conflict checks.
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
}

impl<P, S> std::fmt::Debug for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("settings", &"<ProtocolSettings>")
            .field("storage", &"<Store>")
            .field("wallets", &self.wallets)
            .field("blockchain", &"BlockchainHandle")
            .field("network", &"NetworkHandle")
            .field("sync_import_pipeline", &self.sync_import_pipeline)
            .field("mempool", &self.mempool.total_count())
            .field("header_cache", &self.header_cache.count())
            .field(
                "native_contract_provider_contracts",
                &self.native_contract_provider.all_native_contracts().len(),
            )
            .finish()
    }
}

impl<P, S> Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    /// Returns a fresh [`crate::NodeBuilder`].
    pub fn builder() -> crate::NodeBuilder<P, S> {
        crate::NodeBuilder::default()
    }

    /// Returns the protocol settings the node is running with.
    ///
    /// Convenience accessor for services that received the typed node handle
    /// and need to inspect the network magic, hardfork schedule, etc.
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

    /// Returns the composed sync import pipeline handle.
    pub fn sync_import_pipeline(
        &self,
    ) -> Arc<SyncImportPipeline<SharedStoreSyncStageCheckpointStore<S>>> {
        Arc::clone(&self.sync_import_pipeline)
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

    /// Returns the wallet provider.
    pub fn wallets(&self) -> WalletProvider {
        self.wallets.clone()
    }

    /// Returns a handle to the transaction router used to enqueue
    /// outbound transactions for broadcast / persistence.
    ///
    /// The handle admits into the node's shared `neo-mempool`
    /// instance and best-effort relays accepted transactions through
    /// `neo-network`.
    pub fn tx_router_actor(&self) -> TxRouterHandle<P> {
        TxRouterHandle::new(
            Arc::clone(&self.mempool),
            self.network.clone(),
            Arc::clone(&self.settings),
        )
    }
}

impl Node<neo_native_contracts::StandardNativeProvider, MemoryStore> {
    /// Construct a `Node` with the given protocol settings and an
    /// in-memory storage backend. Used by tests and by the
    /// orchestrator's headless mode (no P2P, no consensus, no
    /// engine).
    pub fn new(
        settings: std::sync::Arc<ProtocolSettings>,
        _blockchain: Option<()>,
        _network: Option<()>,
    ) -> Result<Self, crate::error::NodeError> {
        let storage = Arc::new(MemoryStore::new());
        let native_contract_provider =
            Arc::new(neo_native_contracts::StandardNativeProvider::new());
        let (blockchain, _rx) = neo_blockchain::BlockchainHandle::with_capacity();
        let (network, _nrx, _etx) = neo_network::NetworkHandle::channel(8, 8);
        crate::NodeBuilder::default()
            .with_settings(settings)
            .with_storage(storage)
            .with_blockchain(blockchain)
            .with_network(network)
            .with_native_contract_provider(native_contract_provider)
            .build()
    }
}

/// Handle returned by [`Node::tx_router_actor`]. Wires outbound transactions
/// (e.g. oracle responses) into the shared memory pool and broadcasts admitted
/// ones to peers — the reth-style stand-in for C# `system.Blockchain.Tell(tx)`
/// admit-then-relay.
#[derive(Clone)]
pub struct TxRouterHandle<P = neo_native_contracts::StandardNativeProvider>
where
    P: NativeContractProvider,
{
    mempool: Arc<MemoryPool<P>>,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
}

impl<P> TxRouterHandle<P>
where
    P: NativeContractProvider + 'static,
{
    /// Construct a `TxRouterHandle` over the node's shared mempool + network.
    pub fn new(
        mempool: Arc<MemoryPool<P>>,
        network: NetworkHandle,
        settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            mempool,
            network,
            settings,
        }
    }

    /// Admit `tx` into the shared memory pool against `snapshot`, and — when
    /// `relay` is set and admission succeeded — best-effort broadcast it to
    /// peers. Synchronous and non-blocking, so it is safe to call from a plugin
    /// holding a non-async lock. Returns `Ok(())` only when the mempool accepts
    /// the transaction (`VerifyResult::Succeed`); any other verdict is surfaced
    /// as `Err(verdict)` so the caller can log and retain the work.
    pub fn try_enqueue_preverify<B: neo_storage::CacheRead>(
        &self,
        tx: Transaction,
        relay: bool,
        snapshot: &DataCache<B>,
    ) -> CoreResult<()> {
        let hash = tx
            .try_hash()
            .map_err(|_| CoreError::other(format!("{:?}", VerifyResult::Invalid)))?;
        let ledger = NativeTxAdmissionLedgerProviderFactory.provider(snapshot);
        // Fail closed on a storage error: a transient lookup failure must NOT be
        // treated as "not present" (which would admit and relay a possibly-
        // duplicate transaction). Propagate the error so admission is blocked.
        if ledger
            .contains_transaction(&hash)
            .map_err(|error| CoreError::other(format!("ledger contains_transaction: {error}")))?
        {
            return Err(CoreError::other(format!(
                "{:?}",
                VerifyResult::AlreadyExists
            )));
        }
        let native = NativeTxAdmissionProvider::new(self.mempool.native_contract_provider());
        let max_traceable_blocks = native
            .max_traceable_blocks(snapshot, self.settings.as_ref())
            .map_err(|error| CoreError::other(format!("MaxTraceableBlocks: {error}")))?;
        let signers: Vec<_> = tx.signers().iter().map(|s| s.account).collect();
        if ledger
            .contains_conflict_hash(&hash, &signers, max_traceable_blocks)
            .map_err(|error| CoreError::other(format!("ledger contains_conflict_hash: {error}")))?
        {
            return Err(CoreError::other(format!(
                "{:?}",
                VerifyResult::HasConflicts
            )));
        }
        let result = self.mempool.try_add(tx.clone(), snapshot);
        if result != VerifyResult::Succeed {
            return Err(CoreError::other(format!("{result:?}")));
        }
        if relay {
            // Best-effort relay; a dropped broadcast must not undo a successful
            // admission — the tx is in the pool and will be announced via inv
            // on the next gossip cycle.
            let _ = self.network.try_broadcast_transaction(tx);
        }
        Ok(())
    }
}

impl<P, S> StoreProvider for Node<P, S>
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

impl<P, S> ConfigProvider for Node<P, S>
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

impl<P, S> TxAdmission for Node<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn try_enqueue_preverify<B: neo_storage::CacheRead>(
        &self,
        tx: neo_payloads::Transaction,
        relay: bool,
        snapshot: &neo_storage::persistence::DataCache<B>,
    ) -> Result<(), neo_runtime::ServiceError> {
        self.tx_router_actor()
            .try_enqueue_preverify(tx, relay, snapshot)
            .map_err(|e| neo_runtime::ServiceError::Internal(e.to_string()))
    }
}

#[cfg(test)]
#[path = "../tests/composition/node.rs"]
mod tests;

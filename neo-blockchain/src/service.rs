//! Canonical [`BlockchainService`] implementation.
//!
//! The service is constructed via [`BlockchainService::new`], which
//! returns a `(service, handle)` pair. The handle is what other
//! subsystems store; the service is moved into a `tokio::spawn`'d
//! task that drives the command loop in [`BlockchainService::run`].
//!
//! ## Why two layers?
//!
//! The blockchain is the only subsystem in the node that has
//! command-shaped semantics rather than method-shaped. Every state
//! mutation is funnelled through a single async command loop so the
//! loop can serialise concurrent callers (consensus driver, RPC
//! submit, network inventory, reverify ticker) without an actor
//! framework. The companion [`crate::BlockchainHandle`] is the
//! cheap-to-clone facade the rest of the node uses to talk to the
//! loop.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use neo_primitives::verify_result::VerifyResult;
use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};

use crate::command::BlockchainCommand;
use crate::handle::BlockchainHandle;
use crate::header_cache::HeaderCache;
use crate::internal::UnverifiedBlocksList;
use crate::ledger_context::LedgerContext;
use crate::service_context::SystemContext;

/// Backwards-compatible alias for the legacy actor name.
///
/// The old `neo_core::ledger::blockchain::Blockchain` was an
/// Akka-style actor. The new struct is a reth-style service; the
/// name is kept around so old `use Blockchain;` statements keep
/// resolving.
pub type Blockchain = BlockchainService;

// `AddTransactionReply` is re-exported from `crate::command` for
// downstream callers; the service uses it through that re-export.
pub use crate::command::AddTransactionReply as _AddTransactionReplyAlias;

/// Reth-style blockchain service.
///
/// The service owns the command channel (mpsc), the event channel
/// (broadcast), and the heavy state the legacy actor used to hold
/// (ledger context, header cache, mempool handle, …). Construction
/// goes through [`BlockchainService::new`], which returns the
/// `(service, handle)` pair; the service is moved into a
/// `tokio::spawn`'d task that calls [`BlockchainService::run`].
pub struct BlockchainService {
    /// System context: trait object giving the service access to the
    /// ledger, the mempool, the storage backend, and the network
    /// event stream. The trait is implemented by `neo_core::neo_system`.
    pub(crate) system: Arc<dyn SystemContext>,
    /// In-memory ledger cache. The actor's old struct held this
    /// directly; we keep it on the service so a single
    /// `Arc<BlockchainService>` is still the only owner of the
    /// canonical ledger state.
    pub(crate) ledger: Arc<LedgerContext>,
    /// Header cache (headers received ahead of their blocks).
    pub(crate) header_cache: Arc<HeaderCache>,
    /// Command receiver half. The producer end lives on the
    /// [`BlockchainHandle`].
    pub(crate) cmd_rx: mpsc::Receiver<BlockchainCommand>,
    /// Event broadcast sender. The actor's old implementation
    /// published events through the actor-system event stream; the
    /// new implementation publishes through a `broadcast::Sender`
    /// that subscribers (RPC server, plugins, …) attach to via
    /// [`BlockchainHandle::subscribe`].
    pub(crate) event_tx: broadcast::Sender<crate::RuntimeEvent>,
    /// Mempool access (used by the high-level `add_transaction` API).
    pub(crate) mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>>,
    /// Future blocks grouped by index until their parent is persisted.
    pub(crate) unverified_blocks: Arc<Mutex<BTreeMap<u32, UnverifiedBlocksList>>>,
}

impl fmt::Debug for BlockchainService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockchainService")
            .field("ledger_height", &self.ledger.current_height())
            .field("header_cache_count", &self.header_cache.count())
            .field("unverified_block_count", &self.unverified_block_count())
            .field("cmd_capacity", &self.cmd_rx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl BlockchainService {
    /// Construct a fresh `(service, handle)` pair.
    ///
    /// `cmd_capacity` and `event_capacity` set the sizes of the
    /// mpsc command queue and the broadcast event queue
    /// respectively. Use [`crate::blockchain::DEFAULT_COMMAND_CAPACITY`]
    /// and [`crate::blockchain::DEFAULT_EVENT_CAPACITY`] for the
    /// default sizes.
    pub fn new(
        system: Arc<dyn SystemContext>,
        ledger: Arc<LedgerContext>,
        header_cache: Arc<HeaderCache>,
        mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>>,
        cmd_capacity: usize,
        event_capacity: usize,
    ) -> (Self, BlockchainHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(cmd_capacity);
        let (event_tx, _event_rx) = broadcast::channel(event_capacity);
        let handle = BlockchainHandle {
            cmd_tx,
            event_tx: event_tx.clone(),
        };
        let service = Self {
            system,
            ledger,
            header_cache,
            cmd_rx,
            event_tx,
            mempool,
            unverified_blocks: Arc::new(Mutex::new(BTreeMap::new())),
        };
        (service, handle)
    }

    pub(crate) fn unverified_block_count(&self) -> usize {
        self.unverified_blocks
            .lock()
            .values()
            .map(UnverifiedBlocksList::len)
            .sum()
    }

    /// Convenience constructor that uses the default channel
    /// capacities from the runtime crate.
    pub fn with_defaults(
        system: Arc<dyn SystemContext>,
        ledger: Arc<LedgerContext>,
        header_cache: Arc<HeaderCache>,
        mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>>,
    ) -> (Self, BlockchainHandle) {
        Self::new(
            system,
            ledger,
            header_cache,
            mempool,
            crate::blockchain::DEFAULT_COMMAND_CAPACITY,
            crate::blockchain::DEFAULT_EVENT_CAPACITY,
        )
    }

    /// Drive the service loop until the command channel is closed.
    ///
    /// Every command is dispatched to a synchronous handler method
    /// on the service struct; the loop itself is just
    /// `while let Some(cmd) = self.cmd_rx.recv().await`. This is the
    /// equivalent of the legacy actor's `handle()` method, but
    /// expressed as a normal `async fn` rather than a trait object.
    pub async fn run(mut self) {
        tracing::debug!(target: "neo", "blockchain service run loop started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            self.dispatch(cmd).await;
        }
        tracing::debug!(target: "neo", "blockchain service run loop exited");
    }

    /// Dispatch a single command to its handler. Public for testing
    /// — production callers go through [`Self::run`].
    pub async fn dispatch(&mut self, cmd: BlockchainCommand) {
        match cmd {
            BlockchainCommand::PersistCompleted(persist) => {
                self.handle_persist_completed(persist).await;
            }
            BlockchainCommand::Import(import) => {
                self.handle_import(import).await;
            }
            BlockchainCommand::FillMemoryPool(fill) => {
                self.handle_fill_memory_pool(fill).await;
            }
            BlockchainCommand::FillCompleted => {}
            BlockchainCommand::Reverify(reverify) => {
                self.handle_reverify(reverify).await;
            }
            BlockchainCommand::InventoryBlock {
                block,
                relay,
                pre_verified,
            } => {
                if let Err(error) = self
                    .handle_block_inventory(block, relay, pre_verified)
                    .await
                {
                    tracing::warn!(target: "neo", %error, "inventory block rejected");
                }
            }
            BlockchainCommand::ImportBlock { block, reply } => {
                let before_height = self.ledger.current_height();
                let result = self.handle_block_inventory(block, false, false).await;
                let imported = result.is_ok() && self.ledger.current_height() > before_height;
                if let Err(error) = result {
                    tracing::warn!(target: "neo", %error, "import block rejected");
                }
                let _ = reply.send(imported);
            }
            BlockchainCommand::InventoryExtensible { payload, relay } => {
                let _ = self.handle_extensible_inventory(payload, relay).await;
            }
            BlockchainCommand::PreverifyCompleted(preverify) => {
                self.handle_preverify_completed(preverify).await;
            }
            BlockchainCommand::Headers(headers) => {
                self.handle_headers(headers);
            }
            BlockchainCommand::Idle => {
                self.handle_idle().await;
            }
            BlockchainCommand::DrainUnverified => {
                self.handle_drain_unverified().await;
            }
            BlockchainCommand::RelayResult(result) => {
                self.handle_relay_result(result).await;
            }
            BlockchainCommand::Initialize => {
                self.initialize().await;
            }
            BlockchainCommand::AddTransaction { transaction, reply } => {
                let _ = reply.send(self.add_transaction(transaction).await);
            }
            BlockchainCommand::GetHeight { reply } => {
                let _ = reply.send(self.ledger.current_height());
            }
            BlockchainCommand::GetBlock { hash, reply } => {
                let block = self
                    .ledger
                    .get_block(&hash)
                    .or_else(|| self.full_block_from_store(&hash));
                let _ = reply.send(block);
            }
            BlockchainCommand::GetBlockByHeight { height, reply } => {
                let block = self.ledger.get_block_by_height(height).or_else(|| {
                    self.block_hash_from_store(height)
                        .and_then(|hash| self.full_block_from_store(&hash))
                });
                let _ = reply.send(block);
            }
        }
    }

    /// Resolve a block hash from the durable store for a height, when a
    /// store snapshot is available (cold read after LRU eviction).
    fn block_hash_from_store(&self, height: u32) -> Option<neo_primitives::UInt256> {
        let snapshot = self.system.store_snapshot()?;
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, height)
            .ok()
            .flatten()
    }

    /// Reconstruct a full block from the durable `LedgerContract` trimmed
    /// block plus its per-transaction records (C# `LedgerContract.GetBlock`),
    /// used when the in-memory LRU has evicted the body. Returns `None` when
    /// there is no store, no trimmed block, or any referenced transaction is
    /// missing.
    fn full_block_from_store(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::block::Block> {
        let snapshot = self.system.store_snapshot()?;
        let ledger = neo_native_contracts::LedgerContract::new();
        let trimmed = ledger.get_trimmed_block(&snapshot, hash).ok().flatten()?;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let tx = ledger
                .get_transaction_state(&snapshot, tx_hash)
                .ok()
                .flatten()
                .and_then(|state| state.transaction)?;
            transactions.push(tx);
        }
        Some(neo_payloads::block::Block {
            header: trimmed.header,
            transactions,
        })
    }
}

/// Minimal mempool facade used by the high-level service API.
///
/// The trait exists so the blockchain service can be unit-tested
/// with a mock mempool; the production implementation forwards to the real
/// `MemoryPool` type. The shape is intentionally tiny — the full mempool surface (verification context,
/// conflict attribute detection, reverify queue) lives in
/// `neo-mempool` and is exposed by the [`SystemContext`] trait.
pub trait MempoolLike: std::fmt::Debug + Send + Sync {
    /// Try to add a transaction to the mempool. Returns the verify
    /// result.
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult;

    /// Update the pool after `block` is persisted (C# `MemoryPool.
    /// UpdatePoolForBlockPersisted`): remove the block's transactions and evict
    /// pooled transactions that conflict with the persisted ones. Default no-op
    /// for test mocks without a real pool.
    fn block_persisted(&self, _block: &neo_payloads::Block) {}
}

/// Production [`MempoolLike`] over the real [`neo_mempool::MemoryPool`]:
/// admission runs the full C# `Transaction.Verify` pipeline
/// (`neo_mempool::verification`) against the provided snapshot. The
/// pool owns its protocol settings (taken at construction), matching
/// C# `MemoryPool`'s `_system.Settings` access.
impl MempoolLike for neo_mempool::MemoryPool {
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add(self, tx.clone(), snapshot)
    }

    fn block_persisted(&self, block: &neo_payloads::Block) {
        let _ = self.update_pool_for_block_persisted(&block.transactions);
    }
}

/// Shared-pool adapter so a composition root can hand the *same*
/// `Arc<MemoryPool>` to both the blockchain service (admission) and
/// the node (RPC reads), mirroring how C# `NeoSystem.MemPool` is
/// shared. A newtype rather than `impl MempoolLike for Arc<MemoryPool>`
/// so the trait method does not shadow the pool's inherent `try_add`
/// for `Arc<MemoryPool>` callers.
#[derive(Debug)]
pub struct SharedMempool(pub Arc<neo_mempool::MemoryPool>);

impl MempoolLike for SharedMempool {
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add(&self.0, tx.clone(), snapshot)
    }

    fn block_persisted(&self, block: &neo_payloads::Block) {
        let _ = self.0.update_pool_for_block_persisted(&block.transactions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::BlockchainHandle;
    use std::sync::Arc;

    /// Trivial in-memory mempool used by the unit tests.
    #[derive(Debug, Default)]
    struct TestMempool;

    impl MempoolLike for TestMempool {
        fn try_add(
            &self,
            _tx: &neo_payloads::Transaction,
            _snapshot: &neo_storage::DataCache,
            _settings: &neo_config::ProtocolSettings,
        ) -> VerifyResult {
            VerifyResult::Succeed
        }
    }

    /// Stub system context used by the unit tests.
    #[derive(Debug)]
    struct TestContext;

    impl crate::service_context::SystemContext for TestContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::new(neo_config::ProtocolSettings::default())
        }

        fn current_height(&self) -> u32 {
            0
        }
    }

    #[tokio::test]
    async fn run_loop_processes_simple_command() {
        let system: Arc<dyn crate::service_context::SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        let (service, handle) =
            BlockchainService::with_defaults(system, ledger, header_cache, mempool);

        let task = tokio::spawn(service.run());

        // GetHeight command.
        let height = handle.get_height().await.expect("get_height");
        assert_eq!(height, 0);

        // GetBlock for an unknown hash returns None.
        let hash = neo_primitives::UInt256::zero();
        let block = handle.get_block(&hash).await.expect("get_block");
        assert!(block.is_none());

        // Drop the handle to close the channel; the run loop should exit.
        drop(handle);
        task.await.expect("service task");
    }

    #[test]
    fn handle_debug_includes_capacity() {
        let (handle, _rx) = BlockchainHandle::with_capacity();
        let s = format!("{:?}", handle);
        assert!(s.contains("BlockchainHandle"));
    }

    #[test]
    fn service_debug_does_not_panic() {
        let system: Arc<dyn crate::service_context::SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        let (service, _handle) =
            BlockchainService::with_defaults(system, ledger, header_cache, mempool);
        let s = format!("{:?}", service);
        assert!(s.contains("BlockchainService"));
    }
}

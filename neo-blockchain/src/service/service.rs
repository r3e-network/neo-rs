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

// `AddTransactionReply` is re-exported from `crate::command` for
// downstream callers; the service uses it through that re-export.
pub use crate::command::AddTransactionReply as _AddTransactionReplyAlias;

/// Reth-style blockchain service.
///
/// The service owns the command channel (mpsc), the event channel
/// (broadcast), and the node-service state required for canonical ledger
/// progression (ledger context, header cache, mempool handle, …).
/// Construction goes through [`BlockchainService::new`], which returns the
/// `(service, handle)` pair; the service is moved into a `tokio::spawn`'d
/// task that calls [`BlockchainService::run`].
pub struct BlockchainService<S, M> {
    /// System context giving the service access to the ledger, the
    /// mempool, the storage backend, and lifecycle hooks. The
    /// production implementation is provided by `neo-node`.
    pub(crate) system: Arc<S>,
    /// In-memory ledger cache owned by the service loop.
    pub(crate) ledger: Arc<LedgerContext>,
    /// Header cache (headers received ahead of their blocks).
    pub(crate) header_cache: Arc<HeaderCache>,
    /// Command receiver half. The producer end lives on the
    /// [`BlockchainHandle`].
    pub(crate) cmd_rx: mpsc::Receiver<BlockchainCommand>,
    /// Event broadcast sender that subscribers (RPC server, plugins, …) attach
    /// to via [`BlockchainHandle::subscribe`].
    pub(crate) event_tx: broadcast::Sender<crate::RuntimeEvent>,
    /// Mempool access (used by the high-level `add_transaction` API).
    pub(crate) mempool: Arc<M>,
    /// Future blocks grouped by index until their parent is persisted.
    pub(crate) unverified_blocks: Arc<Mutex<BTreeMap<u32, UnverifiedBlocksList>>>,
    /// Optional validation-mode upper bound for persisted blocks.
    pub(crate) stop_at_height: Option<u32>,
}

impl<S, M> fmt::Debug for BlockchainService<S, M>
where
    S: SystemContext,
    M: MempoolLike,
{
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

impl<S, M> BlockchainService<S, M>
where
    S: SystemContext,
    M: MempoolLike,
{
    /// Construct a fresh `(service, handle)` pair.
    ///
    /// `cmd_capacity` and `event_capacity` set the sizes of the
    /// mpsc command queue and the broadcast event queue
    /// respectively. Use [`crate::blockchain::DEFAULT_COMMAND_CAPACITY`]
    /// and [`crate::blockchain::DEFAULT_EVENT_CAPACITY`] for the
    /// default sizes.
    pub fn new(
        system: Arc<S>,
        ledger: Arc<LedgerContext>,
        header_cache: Arc<HeaderCache>,
        mempool: Arc<M>,
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
            stop_at_height: None,
        };
        (service, handle)
    }

    /// Configure an optional validation stop height.
    pub fn set_stop_at_height(&mut self, stop_at_height: Option<u32>) {
        self.stop_at_height = stop_at_height;
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
        system: Arc<S>,
        ledger: Arc<LedgerContext>,
        header_cache: Arc<HeaderCache>,
        mempool: Arc<M>,
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
    /// `while let Some(cmd) = self.cmd_rx.recv().await`, expressed as a normal
    /// `async fn` over typed channels.
    ///
    /// After processing the first command, drains ALL pending commands in the
    /// channel without awaiting between them. This is critical for sync
    /// throughput: when 500+ blocks arrive in a batch, processing them
    /// one-at-a-time with an `await` yield between each causes the unverified
    /// block cache to overflow and drop blocks. Batching the drain keeps the
    /// cache from filling and sustains network-speed processing.
    pub async fn run(mut self) {
        tracing::debug!(target: "neo", "blockchain service run loop started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            self.dispatch(cmd).await;
            // Drain all remaining pending commands without yielding to the
            // runtime — keeps the pipeline full during catch-up bursts.
            // Bounded to 128 commands per batch to prevent starving other
            // async tasks during sustained catch-up. The yield point gives
            // the runtime a chance to schedule network I/O, consensus ticks,
            // and other services.
            const MAX_DRAIN_PER_BATCH: u32 = 128;
            let mut drained = 0u32;
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                self.dispatch(cmd).await;
                drained += 1;
                if drained >= MAX_DRAIN_PER_BATCH {
                    // Yield to the runtime so other async tasks can make
                    // progress, then resume draining if there are more
                    // pending commands in the channel.
                    tokio::task::yield_now().await;
                    drained = 0;
                }
            }
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
                // Import commands without a reply channel still produce a reply
                // containing error information. Log errors to avoid silently
                // discarding import failures.
                let reply = self.handle_import(import).await;
                if let Some(ref err) = reply.error {
                    tracing::warn!(
                        target: "neo",
                        error = %err,
                        imported = reply.imported,
                        "blockchain import completed with error"
                    );
                }
            }
            BlockchainCommand::ImportBlocks { import, reply } => {
                let result = self.handle_import(import).await;
                let _ = reply.send(result);
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
            BlockchainCommand::InventoryBlocks {
                blocks,
                relay,
                pre_verified,
            } => {
                self.handle_block_inventory_batch(blocks, relay, pre_verified)
                    .await;
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
    ///
    /// Routes through [`StorageLedgerProvider`] (the crate's sole ledger read
    /// path) instead of hand-rolling the native-contract call. The provider's
    /// `block_hash_by_index` is a direct `LedgerContract::get_block_hash`
    /// forward, so collapsing its `CoreResult` with `.ok().flatten()` preserves
    /// the prior "error becomes `None`" semantics byte-for-byte.
    fn block_hash_from_store(&self, height: u32) -> Option<neo_primitives::UInt256> {
        use crate::ledger_provider::BlockProvider;
        let snapshot = self.system.store_snapshot()?;
        crate::ledger_provider::StorageLedgerProvider::new(snapshot.as_ref())
            .block_hash_by_index(height)
            .ok()
            .flatten()
    }

    /// Reconstruct a full block from the durable `LedgerContract` trimmed
    /// block plus its per-transaction records (C# `LedgerContract.GetBlock`),
    /// used when the in-memory LRU has evicted the body. Returns `None` when
    /// there is no store, no trimmed block, or any referenced transaction is
    /// missing.
    ///
    /// Routes through [`StorageLedgerProvider::block_by_hash`], which performs
    /// the identical trimmed-block + per-transaction reconstruction. A missing
    /// referenced transaction makes the provider return `Err`; collapsing with
    /// `.ok().flatten()` maps that to `None`, matching the prior behaviour
    /// where the `?` on the missing transaction short-circuited to `None`.
    fn full_block_from_store(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::block::Block> {
        use crate::ledger_provider::BlockProvider;
        let snapshot = self.system.store_snapshot()?;
        crate::ledger_provider::StorageLedgerProvider::new(snapshot.as_ref())
            .block_by_hash(hash)
            .ok()
            .flatten()
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

    /// Try to add a transaction using a cached state-independent
    /// verification result. When `cached_state_independent` is
    /// `Some(VerifyResult::Succeed)` the mempool skips redundant
    /// signature verification and only performs state-dependent
    /// checks. Should only be used when the caller has already
    /// verified the transaction's signatures (e.g. through
    /// `TransactionRouter::preverify`).
    fn try_add_cached(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        settings: &neo_config::ProtocolSettings,
        cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult;

    /// Update the pool after `block` is persisted (C# `MemoryPool.
    /// UpdatePoolForBlockPersisted`): remove the block's transactions and evict
    /// pooled transactions that conflict with the persisted ones. Default no-op
    /// for test mocks without a real pool.
    fn block_persisted(&self, _block: &neo_payloads::Block) {}

    /// Returns whether the pool has unverified transactions that could be
    /// promoted after a post-persist snapshot becomes available.
    fn has_unverified_transactions(&self) -> bool {
        false
    }

    /// Reverify the highest-priority unverified transactions against the live
    /// post-persist snapshot. Returns `true` when unverified transactions remain.
    fn reverify_top_unverified(
        &self,
        _snapshot: &neo_storage::DataCache,
        _max_count: usize,
    ) -> bool {
        false
    }
}

impl MempoolLike for neo_mempool::MemoryPool {
    fn try_add(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add(self, tx.clone(), snapshot)
    }

    fn try_add_cached(
        &self,
        tx: &neo_payloads::Transaction,
        snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
        cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        neo_mempool::MemoryPool::try_add_cached(
            self,
            tx.clone(),
            snapshot,
            cached_state_independent,
        )
    }

    fn block_persisted(&self, block: &neo_payloads::Block) {
        let _ = self.update_pool_for_block_persisted(&block.transactions);
    }

    fn has_unverified_transactions(&self) -> bool {
        self.unverified_count() > 0
    }

    fn reverify_top_unverified(&self, snapshot: &neo_storage::DataCache, max_count: usize) -> bool {
        neo_mempool::MemoryPool::reverify_top_unverified(self, snapshot, max_count)
    }
}

#[cfg(test)]
#[path = "../tests/service/service.rs"]
mod tests;

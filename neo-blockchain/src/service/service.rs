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

mod dispatch;
mod run_loop;
mod store_reads;

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

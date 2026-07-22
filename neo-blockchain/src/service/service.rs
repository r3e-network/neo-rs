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

use std::fmt;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};

use crate::command::BlockchainCommand;
use crate::handle::BlockchainHandle;
use crate::header_cache::HeaderCache;
use crate::internal::UnverifiedBlockCache;
use crate::ledger_context::LedgerContext;
use crate::pipeline::signature_verification::SignatureVerificationPool;
use crate::service::MempoolLike;
use crate::service_context::SystemContext;

mod dispatch;
mod run_loop;
mod store_reads;

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
    pub(crate) unverified_blocks: Mutex<UnverifiedBlockCache>,
    /// Optional validation-mode upper bound for persisted blocks.
    pub(crate) stop_at_height: Option<u32>,
    /// Optional bounded pool for header witness preverification.
    pub(crate) optimistic_signature_verification: Option<Arc<SignatureVerificationPool>>,
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
            unverified_blocks: Mutex::new(UnverifiedBlockCache::default()),
            stop_at_height: None,
            optimistic_signature_verification: None,
        };
        (service, handle)
    }

    /// Configure an optional validation stop height.
    pub fn set_stop_at_height(&mut self, stop_at_height: Option<u32>) {
        self.stop_at_height = stop_at_height;
    }

    /// Enables the explicitly configured bounded header-witness verifier.
    ///
    /// Workers only produce advisory exact-input ECDSA caches. Canonical NeoVM
    /// header verification and persistence remain on this service's ordered lane.
    pub fn set_optimistic_signature_verification(
        &mut self,
        pool: Option<Arc<SignatureVerificationPool>>,
    ) {
        self.optimistic_signature_verification = pool;
    }

    pub(crate) fn unverified_block_count(&self) -> usize {
        self.unverified_blocks.lock().len()
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

#[cfg(test)]
#[path = "../tests/service/cold_reads.rs"]
mod cold_read_tests;
#[cfg(test)]
#[path = "../tests/service/service.rs"]
mod tests;

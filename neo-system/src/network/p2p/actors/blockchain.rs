//! `Blockchain` chain-sink coordination seam (dest-only, p2p-side).
//!
//! # Why this is *not* a port of the ledger blockchain actor
//!
//! `neo-core::ledger::blockchain` is a ledger/persistence/smart-contract
//! subsystem: it validates blocks against canonical rules, executes
//! transactions, commits to storage, and drives the mempool. Every one of those
//! collaborators (`ledger`, `native`, `storage`, `smart-contract`) is
//! **prohibited** inside `neo-p2p`, so the real blockchain actor cannot and must
//! not be migrated here.
//!
//! What the P2P runtime *does* need is the **consumer side of
//! [`ChainSinkPort`](crate::network::p2p::ports::ChainSinkPort)**: a coordination point that
//! receives the inventory a [`RemoteNode`](super::remote_node::RemoteNode)
//! forwarded, applies purely p2p-local concerns (dedup, consecutive-order
//! buffering, byte/count budget, drain-on-stall), and hands ready-to-commit items
//! to an adapter-provided persistence backend. That backend — the real ledger —
//! lives behind the [`ChainPersistenceSink`] seam and publishes neutral
//! [`PersistenceEvent`]s back out through [`PersistenceEventPublisher`], which is
//! exactly the stream the [`TaskManager`](super::task_manager::TaskManager)
//! subscribes to via
//! [`PersistenceEventPort`](crate::network::p2p::ports::PersistenceEventPort).
//!
//! This keeps `neo-p2p` free of any ledger/native/storage dependency while
//! preserving the ordering and stall-recovery behaviour the original
//! `BlockchainCommand::{InventoryBlock, Headers, DrainUnverified}` path provided.
//!
//! # Task-89 alignment points
//!
//! * [`Actor`] / [`ActorContext`] / [`ScheduleHandle`] are the local placeholder
//!   runtime from [`super`]; task 89 swaps in `neo-system::runtime`.
//! * [`ChainPersistenceSink`] and [`PersistenceEventPublisher`] are adapter
//!   seams, not task-105 ports. Once the ledger lane lands they may graduate
//!   into ports or be folded into the chain actor the adapter already owns; the
//!   coordination state below is written so only those two `Arc<dyn ..>` fields
//!   would change.

use super::{Actor, ActorContext, ActorResult, ScheduleHandle};
use crate::network::p2p::ports::{
    PeerHandle, PersistFailure, PersistedBlock, PersistenceEvent, PortResult, RelayOutcome,
};
use async_trait::async_trait;
use neo_io_crate::{HashSetCache, Serializable};
use neo_primitives::UInt256;
use neo_protocol::payloads::{Block, Extensible, Header};
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Interval at which the coordination seam re-attempts to drain its ordered
/// window. Mirrors `neo-core`'s `DRAIN_TIMER_INTERVAL`: without it a stall can
/// persist when the block that would unblock the frontier is delayed.
const DRAIN_TIMER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// Maximum number of out-of-order blocks parked awaiting the frontier.
const MAX_PENDING_BLOCKS: usize = 256;
/// Maximum serialized bytes parked awaiting the frontier.
const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
/// Dedup cache sizing for recently submitted inventory hashes.
const KNOWN_HASH_CAPACITY: usize = 4096;

// ===========================================================================
// Adapter seams (the real ledger lives behind these; not task-105 ports).
// ===========================================================================

/// Adapter-provided validate/commit backend (the real ledger blockchain).
///
/// The coordination seam never validates canonical rules itself; it only decides
/// *when* an item is ready to commit (consecutive with the frontier) and
/// forwards it here. A non-`Ok` result means the item was **not** committed, so
/// the seam publishes a [`PersistFailure`] and stops draining rather than
/// advancing the frontier on an uncommitted height.
pub trait ChainPersistenceSink: Send + Sync {
    /// Validates and commits one block, returning the committed index.
    fn persist_block(
        &self,
        block: Block,
        relay: bool,
        pre_verified: bool,
        source: PeerHandle,
    ) -> PortResult<u32>;

    /// Accepts one extensible payload (dBFT / state-service items).
    fn persist_extensible(
        &self,
        payload: Extensible,
        relay: bool,
        source: PeerHandle,
    ) -> PortResult<()>;

    /// Stores a header batch, returning the new highest header index.
    fn persist_headers(&self, headers: Vec<Header>, source: PeerHandle) -> PortResult<u32>;
}

/// Adapter-provided publisher of neutral persistence/relay events.
///
/// This is the source side of the
/// [`PersistenceEventPort`](crate::network::p2p::ports::PersistenceEventPort) the task manager
/// subscribes to. Publishing keeps the frontier advance observable without the
/// P2P layer naming any ledger/system event type.
pub trait PersistenceEventPublisher: Send + Sync {
    /// Publishes one event to every subscriber.
    fn publish(&self, event: PersistenceEvent) -> PortResult<()>;
}

// ===========================================================================
// Command vocabulary.
// ===========================================================================

/// Commands accepted by the [`Blockchain`] coordination seam.
///
/// A p2p-local subset of `neo-core`'s `BlockchainCommand`: only the variants
/// that cross the chain-sink boundary are modelled. The ledger-owned variants
/// (`Import`, `FillMemoryPool`, `Reverify`, `PreverifyCompleted`,
/// `AttachSystem`) are deliberately absent because they belong to the prohibited
/// ledger subsystem.
#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    /// One-time initialization (seed the frontier from the backend).
    Initialize,
    /// A peer-forwarded block awaiting validation/commit.
    InventoryBlock {
        /// The received block.
        block: Block,
        /// Whether to relay once accepted.
        relay: bool,
        /// Whether state-independent verification already ran.
        pre_verified: bool,
        /// The peer that delivered the block.
        source: PeerHandle,
    },
    /// A peer-forwarded extensible payload.
    InventoryExtensible {
        /// The extensible payload.
        payload: Extensible,
        /// Whether to relay once accepted.
        relay: bool,
        /// The peer that delivered the payload.
        source: PeerHandle,
    },
    /// A peer-forwarded header batch.
    Headers {
        /// The received headers.
        headers: Vec<Header>,
        /// The peer that delivered the headers.
        source: PeerHandle,
    },
    /// External notification that a block committed (advances the frontier).
    Persisted(PersistedBlock),
    /// External notification that a verified block failed to commit.
    Failed(PersistFailure),
    /// External notification of a relay outcome.
    Relay(RelayOutcome),
    /// Timer tick: re-attempt to drain the ordered window.
    DrainUnverified,
    /// Background idle tick.
    Idle,
}

// ===========================================================================
// Coordination state.
// ===========================================================================

/// The p2p-side chain-sink coordination actor.
///
/// Buffers out-of-order blocks, commits them consecutively through the
/// [`ChainPersistenceSink`], and republishes neutral events so the task manager
/// can advance its persist frontier. Holds no ledger/native/storage state.
pub struct Blockchain {
    persistence: Arc<dyn ChainPersistenceSink>,
    events: Arc<dyn PersistenceEventPublisher>,
    /// Highest committed block index observed.
    frontier: u32,
    /// Highest stored header index observed.
    header_tip: u32,
    /// Ordered window of blocks parked awaiting the frontier.
    pending: BTreeMap<u32, Block>,
    /// Approximate serialized bytes held in [`Self::pending`].
    pending_bytes: usize,
    /// Recently submitted inventory hashes (dedup).
    known: HashSetCache<UInt256>,
    drain_timer: Option<ScheduleHandle>,
    initialized: bool,
}

impl Blockchain {
    /// Constructs the coordination seam over its two adapter seams.
    pub fn new(
        persistence: Arc<dyn ChainPersistenceSink>,
        events: Arc<dyn PersistenceEventPublisher>,
    ) -> Self {
        Self {
            persistence,
            events,
            frontier: 0,
            header_tip: 0,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            known: HashSetCache::new(KNOWN_HASH_CAPACITY),
            drain_timer: None,
            initialized: false,
        }
    }

    /// The highest committed block index observed.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    /// The highest stored header index observed.
    pub fn header_tip(&self) -> u32 {
        self.header_tip
    }

    /// Number of blocks currently parked awaiting the frontier.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn budget_exhausted(&self) -> bool {
        self.pending.len() >= MAX_PENDING_BLOCKS || self.pending_bytes >= MAX_PENDING_BYTES
    }

    /// Parks a block in the ordered window, honoring the byte/count budget.
    ///
    /// Blocks at or below the frontier are dropped (already committed). When the
    /// budget is exhausted the highest parked index is evicted so a block nearer
    /// the frontier can always make progress, mirroring the task-session policy.
    fn park_block(&mut self, index: u32, block: Block) -> bool {
        if index <= self.frontier {
            return false;
        }
        let size = block.size();
        if self.pending.contains_key(&index) {
            return false;
        }
        if self.budget_exhausted() {
            // Evict the highest parked index if it is above the incoming block.
            let highest = self.pending.keys().next_back().copied();
            match highest {
                Some(highest) if highest > index => {
                    if let Some(removed) = self.pending.remove(&highest) {
                        self.pending_bytes = self.pending_bytes.saturating_sub(removed.size());
                    }
                }
                _ => return false,
            }
        }
        self.pending.insert(index, block);
        self.pending_bytes = self.pending_bytes.saturating_add(size);
        true
    }

    /// Commits every block consecutive with the frontier, publishing events.
    fn drain(&mut self) {
        loop {
            let next = self.frontier.saturating_add(1);
            let Some(block) = self.pending.remove(&next) else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(block.size());

            let mut hash_block = block.clone();
            let hash = hash_block.try_hash().ok();

            match self
                .persistence
                .persist_block(block, true, false, self.current_source())
            {
                Ok(index) => {
                    if index > self.frontier {
                        self.frontier = index;
                    }
                    if let Some(hash) = hash {
                        self.known.try_add(hash);
                    }
                    let persisted = PersistedBlock {
                        block: self.rebuild_committed(index, hash),
                    };
                    let _ = self.events.publish(PersistenceEvent::Persisted(persisted));
                }
                Err(_) => {
                    if let Some(index) = hash.map(|_| next) {
                        let _ = self
                            .events
                            .publish(PersistenceEvent::Failed(PersistFailure { index }));
                    }
                    // Stop draining: the frontier cannot advance past a failure.
                    break;
                }
            }
        }
    }

    /// Placeholder source handle for adapter-committed blocks.
    ///
    /// The coordination seam does not retain per-block peer provenance (that is
    /// the [`ChainPersistenceSink`] adapter's concern), so commits are attributed
    /// to a neutral handle. Documented as a task-89 alignment point.
    fn current_source(&self) -> PeerHandle {
        PeerHandle::new(0)
    }

    /// Rebuilds a minimal committed-block marker for the event.
    ///
    /// The task manager only reads the committed block's index and hash from a
    /// [`PersistedBlock`]; the adapter that owns the ledger may replace this with
    /// the authoritative block. Kept minimal to avoid retaining full payloads.
    fn rebuild_committed(&self, index: u32, _hash: Option<UInt256>) -> Block {
        let mut block = Block::new();
        block.header.set_index(index);
        block
    }

    fn on_inventory_block(&mut self, mut block: Block, source: PeerHandle) {
        let index = block.index();
        let hash = match block.try_hash() {
            Ok(hash) => hash,
            Err(_) => return,
        };
        if self.known.contains(&hash) || index <= self.frontier {
            return;
        }
        let _ = source;
        self.park_block(index, block);
        self.drain();
    }

    fn on_inventory_extensible(
        &mut self,
        mut payload: Extensible,
        relay: bool,
        source: PeerHandle,
    ) {
        let hash = match payload.try_hash() {
            Ok(hash) => hash,
            Err(_) => return,
        };
        if self.known.contains(&hash) {
            return;
        }
        self.known.try_add(hash);
        let _ = self.persistence.persist_extensible(payload, relay, source);
    }

    fn on_headers(&mut self, headers: Vec<Header>, source: PeerHandle) {
        if headers.is_empty() {
            return;
        }
        if let Ok(tip) = self.persistence.persist_headers(headers, source) {
            if tip > self.header_tip {
                self.header_tip = tip;
            }
        }
    }

    fn on_persisted(&mut self, persisted: PersistedBlock) {
        let index = persisted.block.index();
        if index > self.frontier {
            self.frontier = index;
        }
        // Drop everything at or below the new frontier and re-drain.
        let below: Vec<u32> = self.pending.range(..=index).map(|(key, _)| *key).collect();
        for key in below {
            if let Some(removed) = self.pending.remove(&key) {
                self.pending_bytes = self.pending_bytes.saturating_sub(removed.size());
            }
        }
        let _ = self.events.publish(PersistenceEvent::Persisted(persisted));
        self.drain();
    }

    fn on_failed(&mut self, failure: PersistFailure) {
        let _ = self.events.publish(PersistenceEvent::Failed(failure));
    }

    fn on_relay(&mut self, outcome: RelayOutcome) {
        let _ = self.events.publish(PersistenceEvent::Relay(outcome));
    }
}

#[async_trait]
impl Actor for Blockchain {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        if self.drain_timer.is_none() {
            self.drain_timer = Some(ctx.schedule_repeated(
                DRAIN_TIMER_INTERVAL,
                DRAIN_TIMER_INTERVAL,
                Box::new(BlockchainCommand::DrainUnverified),
            ));
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let Ok(command) = message.downcast::<BlockchainCommand>() else {
            return Ok(());
        };
        match *command {
            BlockchainCommand::Initialize => {
                self.initialized = true;
            }
            BlockchainCommand::InventoryBlock {
                block,
                relay: _,
                pre_verified: _,
                source,
            } => {
                self.on_inventory_block(block, source);
            }
            BlockchainCommand::InventoryExtensible {
                payload,
                relay,
                source,
            } => {
                self.on_inventory_extensible(payload, relay, source);
            }
            BlockchainCommand::Headers { headers, source } => {
                self.on_headers(headers, source);
            }
            BlockchainCommand::Persisted(persisted) => self.on_persisted(persisted),
            BlockchainCommand::Failed(failure) => self.on_failed(failure),
            BlockchainCommand::Relay(outcome) => self.on_relay(outcome),
            BlockchainCommand::DrainUnverified => self.drain(),
            BlockchainCommand::Idle => {}
        }
        let _ = ctx;
        Ok(())
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        if let Some(timer) = self.drain_timer.take() {
            timer.cancel();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records committed indexes so the drain ordering can be asserted.
    #[derive(Default)]
    struct RecordingSink {
        committed: Mutex<Vec<u32>>,
        fail_at: Mutex<Option<u32>>,
        header_tip: Mutex<u32>,
    }

    impl ChainPersistenceSink for RecordingSink {
        fn persist_block(
            &self,
            block: Block,
            _relay: bool,
            _pre_verified: bool,
            _source: PeerHandle,
        ) -> PortResult<u32> {
            let index = block.index();
            if let Some(fail) = self.fail_at.lock().unwrap().as_ref() {
                if *fail == index {
                    return Err(crate::network::p2p::ports::PortError::delivery("commit failed"));
                }
            }
            self.committed.lock().unwrap().push(index);
            Ok(index)
        }

        fn persist_extensible(
            &self,
            _payload: Extensible,
            _relay: bool,
            _source: PeerHandle,
        ) -> PortResult<()> {
            Ok(())
        }

        fn persist_headers(&self, headers: Vec<Header>, _source: PeerHandle) -> PortResult<u32> {
            let tip = headers.iter().map(Header::index).max().unwrap_or(0);
            let mut guard = self.header_tip.lock().unwrap();
            if tip > *guard {
                *guard = tip;
            }
            Ok(*guard)
        }
    }

    #[derive(Default)]
    struct RecordingPublisher {
        events: Mutex<Vec<PersistenceEvent>>,
    }

    impl PersistenceEventPublisher for RecordingPublisher {
        fn publish(&self, event: PersistenceEvent) -> PortResult<()> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    fn block_at(index: u32) -> Block {
        let mut block = Block::new();
        block.header.set_index(index);
        block
    }

    fn harness() -> (Blockchain, Arc<RecordingSink>, Arc<RecordingPublisher>) {
        let sink = Arc::new(RecordingSink::default());
        let publisher = Arc::new(RecordingPublisher::default());
        let chain = Blockchain::new(sink.clone(), publisher.clone());
        (chain, sink, publisher)
    }

    #[test]
    fn drains_consecutive_blocks_in_order() {
        let (mut chain, sink, publisher) = harness();
        // Park out of order; nothing commits until index 1 is present.
        chain.on_inventory_block(block_at(2), PeerHandle::new(1));
        chain.on_inventory_block(block_at(3), PeerHandle::new(1));
        assert_eq!(chain.frontier(), 0);
        assert_eq!(sink.committed.lock().unwrap().len(), 0);

        // Frontier block arrives; the whole run commits consecutively.
        chain.on_inventory_block(block_at(1), PeerHandle::new(1));
        assert_eq!(chain.frontier(), 3);
        assert_eq!(*sink.committed.lock().unwrap(), vec![1, 2, 3]);
        assert_eq!(chain.pending_len(), 0);
        assert_eq!(publisher.events.lock().unwrap().len(), 3);
    }

    #[test]
    fn stops_draining_at_a_commit_failure() {
        let (mut chain, sink, publisher) = harness();
        *sink.fail_at.lock().unwrap() = Some(2);
        chain.on_inventory_block(block_at(1), PeerHandle::new(1));
        chain.on_inventory_block(block_at(2), PeerHandle::new(1));
        chain.on_inventory_block(block_at(3), PeerHandle::new(1));

        // Only index 1 commits; the failure at 2 halts the drain.
        assert_eq!(chain.frontier(), 1);
        assert_eq!(*sink.committed.lock().unwrap(), vec![1]);
        assert!(
            publisher
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, PersistenceEvent::Failed(f) if f.index == 2))
        );
        assert_eq!(chain.pending_len(), 2);
    }

    #[test]
    fn ignores_blocks_at_or_below_the_frontier() {
        let (mut chain, sink, _publisher) = harness();
        chain.on_inventory_block(block_at(1), PeerHandle::new(1));
        assert_eq!(chain.frontier(), 1);
        // A stale/duplicate block must not re-commit.
        chain.on_inventory_block(block_at(1), PeerHandle::new(1));
        chain.on_inventory_block(block_at(0), PeerHandle::new(1));
        assert_eq!(*sink.committed.lock().unwrap(), vec![1]);
    }

    #[test]
    fn headers_advance_the_header_tip() {
        let (mut chain, _sink, _publisher) = harness();
        let mut h1 = Header::new();
        h1.set_index(5);
        let mut h2 = Header::new();
        h2.set_index(9);
        chain.on_headers(vec![h1, h2], PeerHandle::new(1));
        assert_eq!(chain.header_tip(), 9);
    }
}

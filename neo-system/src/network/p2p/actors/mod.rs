//! P2P runtime actor bodies (dest-only migration from `neo-core`).
//!
//! This module hosts the *actor bodies* that used to live inside
//! `neo-core::network::p2p` (`RemoteNode`, `TaskManager`) and the thin
//! blockchain-coordination seam (`Blockchain`). They are re-expressed here so
//! the networking runtime depends only on:
//!
//! * [`crate::ports`] — the task-105 typed port bundles
//!   ([`RemoteSessionPorts`](crate::ports::RemoteSessionPorts) and
//!   [`TaskManagerPorts`](crate::ports::TaskManagerPorts)) plus the opaque
//!   [`PeerHandle`](crate::ports::PeerHandle) that replaces `ActorRef`.
//! * [`crate::sessions`] — the task-103 [`TaskSession`](crate::sessions::TaskSession)
//!   per-peer bookkeeping.
//! * `neo-protocol` canonical payloads (`Block`, `Transaction`, `Extensible`,
//!   `Header`), `neo-primitives`, this crate's own payload vocabulary, `tokio`,
//!   and `std`.
//!
//! The actors deliberately **never** name `NeoSystemContext`, `ActorRef`, a
//! ledger/native/storage type, or the concrete actor runtime. Everything that
//! used to reach "through" the system context now goes through a port trait
//! object, and every peer is addressed by an opaque [`PeerHandle`].
//!
//! # Dest-only status (task 89 alignment points)
//!
//! Like [`crate::ports`] and [`crate::sessions`], this module is **not yet
//! declared from the crate root** and the `neo-protocol` dependency it names is
//! **not yet in the manifest**. Nothing here compiles into `neo-p2p` until task
//! 89:
//!
//! 1. adds `pub mod actors;` (alongside `pub mod ports;` / `pub mod sessions;`)
//!    to `lib.rs`,
//! 2. adds the `neo-protocol` manifest dependency, and
//! 3. replaces the placeholder runtime seam below with the real
//!    `neo-system::runtime` `Actor` / `ActorContext` / `ScheduleHandle`.
//!
//! The placeholder runtime types ([`Actor`], [`ActorContext`], [`ActorResult`],
//! [`ScheduleHandle`], [`ActorScheduler`]) mirror the shape of the
//! `neo-core::runtime` (== `neo-system::runtime`) traits the original actors
//! implemented, so the swap in task 89 is mechanical: the actor bodies only use
//! `self_handle()`, `schedule_once()`, `schedule_repeated()`, and
//! `stop_self()`.
//!
//! # Adapter seams that are *not* task-105 ports
//!
//! Two collaborators of the original `RemoteNode` are peer-registry / wire
//! concerns that task 105 did not model as ports, so they stay behind small
//! local seam traits the embedding adapter implements:
//!
//! * [`OutboundSink`] — the session's own wire write queue (framing/flush is
//!   the adapter's job; the actor only produces decoded
//!   [`SessionOutbound`] values).
//! * [`SessionLifecycleSink`] — the `LocalNode`/peer-registry coordination the
//!   actor previously performed by `tell`-ing its parent (`ConnectionEstablished`,
//!   `ConnectionFailed`, `ConnectionTerminated`, `AddPeers`, height updates,
//!   and the `GetAddr` address book).
//!
//! Both are documented as task-89 alignment points: they may graduate into
//! ports, or be folded into the runtime, once the surrounding lanes land.

pub mod blockchain;
pub mod remote_node;
pub mod task_manager;

pub use blockchain::{Blockchain, BlockchainCommand};
pub use remote_node::{RemoteNode, RemoteNodeCommand};
pub use task_manager::{TaskManager, TaskManagerActor, TaskManagerCommand};

use crate::payloads::{
    AddrPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload, GetBlocksPayload,
    InvPayload, NetworkAddressWithTime, PingPayload, VersionPayload,
};
use crate::ports::PeerHandle;
use async_trait::async_trait;
use neo_protocol::payloads::{Block, Extensible, Header, Transaction};
use std::any::Any;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ===========================================================================
// Placeholder actor runtime seam (task 89 replaces with neo-system::runtime).
// ===========================================================================

/// Result alias returned by every actor lifecycle callback.
pub type ActorResult = Result<(), ActorError>;

/// Minimal error carrier for the placeholder runtime.
///
/// Mirrors the `system(...)` constructor the original actors used from
/// `ActorRuntimeError`; task 89 swaps this for the real runtime error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorError {
    message: String,
}

impl ActorError {
    /// Builds a system-level actor error from any displayable context.
    pub fn system(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The human-readable error context.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "actor error: {}", self.message)
    }
}

impl std::error::Error for ActorError {}

/// Cancellation handle for a scheduled message.
///
/// Placeholder for `neo-system::runtime::ScheduleHandle`. Cancelling is
/// idempotent and inert until the real scheduler is wired in task 89.
#[derive(Debug)]
pub struct ScheduleHandle {
    cancelled: Arc<AtomicBool>,
}

impl ScheduleHandle {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancels the scheduled delivery (idempotent).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Whether this handle has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Clone for ScheduleHandle {
    fn clone(&self) -> Self {
        Self {
            cancelled: Arc::clone(&self.cancelled),
        }
    }
}

/// Scheduler seam the placeholder [`ActorContext`] delegates timer delivery to.
///
/// Task 89 provides the real implementation backed by the actor runtime; the
/// [`NullScheduler`] default keeps the dest-only bodies inert and panic-free.
pub trait ActorScheduler: Send + Sync {
    /// Schedules a one-shot `message` delivery to `target` after `delay`.
    fn schedule_once(
        &self,
        target: PeerHandle,
        delay: Duration,
        message: Box<dyn Any + Send>,
    ) -> ScheduleHandle;

    /// Schedules a repeating `message` delivery to `target`.
    fn schedule_repeated(
        &self,
        target: PeerHandle,
        initial: Duration,
        interval: Duration,
        message: Box<dyn Any + Send>,
    ) -> ScheduleHandle;
}

/// Inert scheduler used until the real runtime is wired (dest-only default).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullScheduler;

impl ActorScheduler for NullScheduler {
    fn schedule_once(
        &self,
        _target: PeerHandle,
        _delay: Duration,
        _message: Box<dyn Any + Send>,
    ) -> ScheduleHandle {
        ScheduleHandle::new()
    }

    fn schedule_repeated(
        &self,
        _target: PeerHandle,
        _initial: Duration,
        _interval: Duration,
        _message: Box<dyn Any + Send>,
    ) -> ScheduleHandle {
        ScheduleHandle::new()
    }
}

/// Per-invocation actor context (placeholder for `neo-system::runtime::ActorContext`).
///
/// Exposes only the surface the migrated bodies need: the actor's own opaque
/// [`PeerHandle`], timer scheduling, and a self-stop request. There is no
/// `ActorRef`, no `parent()`, and no system context.
pub struct ActorContext {
    self_handle: PeerHandle,
    scheduler: Arc<dyn ActorScheduler>,
    stopped: bool,
}

impl ActorContext {
    /// Builds a context for `self_handle` backed by `scheduler`.
    pub fn new(self_handle: PeerHandle, scheduler: Arc<dyn ActorScheduler>) -> Self {
        Self {
            self_handle,
            scheduler,
            stopped: false,
        }
    }

    /// Builds a context backed by the inert [`NullScheduler`] (dest-only/tests).
    pub fn inert(self_handle: PeerHandle) -> Self {
        Self::new(self_handle, Arc::new(NullScheduler))
    }

    /// The opaque handle identifying this actor's own session.
    pub fn self_handle(&self) -> PeerHandle {
        self.self_handle
    }

    /// Schedules a one-shot message back to this actor.
    pub fn schedule_once(&self, delay: Duration, message: Box<dyn Any + Send>) -> ScheduleHandle {
        self.scheduler
            .schedule_once(self.self_handle, delay, message)
    }

    /// Schedules a repeating message back to this actor.
    pub fn schedule_repeated(
        &self,
        initial: Duration,
        interval: Duration,
        message: Box<dyn Any + Send>,
    ) -> ScheduleHandle {
        self.scheduler
            .schedule_repeated(self.self_handle, initial, interval, message)
    }

    /// Requests that the runtime stop this actor after the current message.
    pub fn stop_self(&mut self) -> ActorResult {
        self.stopped = true;
        Ok(())
    }

    /// Whether [`Self::stop_self`] has been requested.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
}

/// Core trait implemented by every migrated actor body.
///
/// Placeholder for `neo-system::runtime::Actor`; the signature (a boxed
/// `Any` envelope plus a mutable context) is intentionally identical so task 89
/// can `impl` the real trait without touching the bodies.
#[async_trait]
pub trait Actor: Send + 'static {
    /// Invoked after start and before the first message is processed.
    async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        Ok(())
    }

    /// Handles a single incoming message envelope.
    async fn handle(&mut self, message: Box<dyn Any + Send>, ctx: &mut ActorContext)
    -> ActorResult;

    /// Invoked before the actor is permanently stopped.
    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        Ok(())
    }
}

// ===========================================================================
// Wire vocabulary (decoded protocol messages crossing the actor boundary).
// ===========================================================================

/// A decoded inbound protocol message offered to a [`RemoteNode`] session.
///
/// The wire adapter (framing/decompression, task 114 / task 89) decodes a frame
/// into this vocabulary before handing it to the actor; the actor never touches
/// raw sockets. Variants name only canonical protocol payloads and this crate's
/// own payload types.
#[derive(Debug, Clone)]
pub enum SessionInbound {
    /// Handshake version advertisement.
    Version(VersionPayload),
    /// Handshake acknowledgement.
    Verack,
    /// Keepalive ping.
    Ping(PingPayload),
    /// Keepalive pong.
    Pong(PingPayload),
    /// Inventory announcement.
    Inv(InvPayload),
    /// Inventory data request.
    GetData(InvPayload),
    /// Block-hash range request.
    GetBlocks(GetBlocksPayload),
    /// Block-index range request.
    GetBlockByIndex(GetBlockByIndexPayload),
    /// Header range request.
    GetHeaders(GetBlockByIndexPayload),
    /// Header batch delivery.
    Headers(Vec<Header>),
    /// Transaction delivery.
    Transaction(Transaction),
    /// Block delivery.
    Block(Block),
    /// Extensible payload delivery.
    Extensible(Extensible),
    /// Mempool snapshot request.
    Mempool,
    /// Peer-address request.
    GetAddr,
    /// Peer-address list.
    Addr(AddrPayload),
    /// Requested inventory unavailable.
    NotFound(InvPayload),
    /// Bloom filter load.
    FilterLoad(FilterLoadPayload),
    /// Bloom filter add.
    FilterAdd(FilterAddPayload),
    /// Bloom filter clear.
    FilterClear,
    /// Reject notification (raw wire body; logging only).
    Reject(Vec<u8>),
    /// Network alert (raw wire body; logging only).
    Alert(Vec<u8>),
}

impl SessionInbound {
    /// The [`crate::MessageCommand`] discriminator for this inbound message.
    pub fn command(&self) -> crate::MessageCommand {
        use crate::MessageCommand;
        match self {
            Self::Version(_) => MessageCommand::Version,
            Self::Verack => MessageCommand::Verack,
            Self::Ping(_) => MessageCommand::Ping,
            Self::Pong(_) => MessageCommand::Pong,
            Self::Inv(_) => MessageCommand::Inv,
            Self::GetData(_) => MessageCommand::GetData,
            Self::GetBlocks(_) => MessageCommand::GetBlocks,
            Self::GetBlockByIndex(_) => MessageCommand::GetBlockByIndex,
            Self::GetHeaders(_) => MessageCommand::GetHeaders,
            Self::Headers(_) => MessageCommand::Headers,
            Self::Transaction(_) => MessageCommand::Transaction,
            Self::Block(_) => MessageCommand::Block,
            Self::Extensible(_) => MessageCommand::Extensible,
            Self::Mempool => MessageCommand::Mempool,
            Self::GetAddr => MessageCommand::GetAddr,
            Self::Addr(_) => MessageCommand::Addr,
            Self::NotFound(_) => MessageCommand::NotFound,
            Self::FilterLoad(_) => MessageCommand::FilterLoad,
            Self::FilterAdd(_) => MessageCommand::FilterAdd,
            Self::FilterClear => MessageCommand::FilterClear,
            Self::Reject(_) => MessageCommand::Reject,
            Self::Alert(_) => MessageCommand::Alert,
        }
    }

    /// Raw payload bytes for the [`crate::ports::InboundDispatchPort`] veto hook.
    ///
    /// The dispatch port only needs a byte view for external handlers; the
    /// dest-only bodies pass an empty slice because re-serializing canonical
    /// payloads here would duplicate the wire adapter's job. Task 89 may thread
    /// the original frame bytes through instead.
    pub fn raw_payload(&self) -> &[u8] {
        &[]
    }
}

/// A decoded outbound protocol message produced by a [`RemoteNode`] session.
#[derive(Debug, Clone)]
pub enum SessionOutbound {
    /// Handshake version advertisement.
    Version(VersionPayload),
    /// Handshake acknowledgement.
    Verack,
    /// Keepalive ping.
    Ping(PingPayload),
    /// Keepalive pong.
    Pong(PingPayload),
    /// Inventory announcement.
    Inv(InvPayload),
    /// Inventory data request.
    GetData(InvPayload),
    /// Block-index range request.
    GetBlockByIndex(GetBlockByIndexPayload),
    /// Header batch delivery.
    Headers(Vec<Header>),
    /// Transaction delivery.
    Transaction(Transaction),
    /// Block delivery.
    Block(Block),
    /// Extensible payload delivery.
    Extensible(Extensible),
    /// Mempool snapshot request.
    Mempool,
    /// Peer-address request.
    GetAddr,
    /// Peer-address list.
    Addr(AddrPayload),
    /// Requested inventory unavailable.
    NotFound(InvPayload),
}

impl SessionOutbound {
    /// The [`crate::MessageCommand`] discriminator for this outbound message.
    pub fn command(&self) -> crate::MessageCommand {
        use crate::MessageCommand;
        match self {
            Self::Version(_) => MessageCommand::Version,
            Self::Verack => MessageCommand::Verack,
            Self::Ping(_) => MessageCommand::Ping,
            Self::Pong(_) => MessageCommand::Pong,
            Self::Inv(_) => MessageCommand::Inv,
            Self::GetData(_) => MessageCommand::GetData,
            Self::GetBlockByIndex(_) => MessageCommand::GetBlockByIndex,
            Self::Headers(_) => MessageCommand::Headers,
            Self::Transaction(_) => MessageCommand::Transaction,
            Self::Block(_) => MessageCommand::Block,
            Self::Extensible(_) => MessageCommand::Extensible,
            Self::Mempool => MessageCommand::Mempool,
            Self::GetAddr => MessageCommand::GetAddr,
            Self::Addr(_) => MessageCommand::Addr,
            Self::NotFound(_) => MessageCommand::NotFound,
        }
    }
}

/// Session wire-write seam (adapter-provided; not a task-105 port).
///
/// The actor enqueues decoded [`SessionOutbound`] values; framing, compression,
/// buffering, and flushing remain the wire adapter's responsibility. A non-`Ok`
/// result means the message was not queued, matching the fail-closed contract
/// the original `send_wire_message` upheld.
pub trait OutboundSink: Send + Sync {
    /// Queues one outbound protocol message for the session's peer.
    fn enqueue(&self, message: SessionOutbound) -> crate::ports::PortResult<()>;
}

// ===========================================================================
// LocalNode / peer-registry coordination seam (adapter-provided).
// ===========================================================================

/// Snapshot of a handshaked peer, mirroring `neo-core`'s `RemoteNodeSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSnapshot {
    /// Remote socket address.
    pub remote_address: SocketAddr,
    /// Remote port.
    pub remote_port: u16,
    /// Advertised TCP listen port.
    pub listen_tcp_port: u16,
    /// Advertised last block index.
    pub last_block_index: u32,
    /// Protocol version.
    pub version: u32,
    /// Capability service mask.
    pub services: u64,
    /// Unix timestamp of the snapshot.
    pub timestamp: u64,
}

/// `LocalNode`/peer-registry coordination seam (adapter-provided).
///
/// Replaces the original `RemoteNode`'s `parent().tell(PeerCommand::..)` calls
/// and `local_node.*` reads. The async connection-authorization handshake is
/// collapsed to a synchronous accept/reject decision, consistent with the
/// synchronous port contract; the adapter may block internally if it needs to
/// consult the registry.
pub trait SessionLifecycleSink: Send + Sync {
    /// Offers an established connection to the registry.
    ///
    /// Returns `true` to accept the peer, `false` to reject it. Mirrors the
    /// `ConnectionEstablished` request/reply the actor awaited on its parent.
    fn connection_established(
        &self,
        handle: PeerHandle,
        snapshot: PeerSnapshot,
        version: &VersionPayload,
        is_trusted: bool,
        inbound: bool,
    ) -> bool;

    /// Reports a failed outbound connection attempt (`ConnectionFailed`).
    fn connection_failed(&self, handle: PeerHandle, endpoint: SocketAddr);

    /// Reports session teardown (`ConnectionTerminated`).
    fn connection_terminated(&self, handle: PeerHandle);

    /// Updates the registry's cached peer height.
    fn update_peer_height(&self, handle: PeerHandle, last_block_index: u32);

    /// Forwards discovered peer endpoints to the registry (`AddPeers`).
    fn add_peers(&self, endpoints: Vec<SocketAddr>);

    /// Returns the address book served for a `GetAddr` request.
    fn address_book(&self) -> Vec<NetworkAddressWithTime>;
}

// ===========================================================================
// Shared session helpers ported from neo-core (dependency-light rewrites).
// ===========================================================================

/// Maximum TTL for pending announced hashes before they are pruned.
pub const PENDING_KNOWN_HASH_TTL: Duration = Duration::from_secs(60);

/// FIFO, TTL-bounded set of recently announced hashes.
///
/// Faithful rewrite of `neo-core`'s `PendingKnownHashes`, which relied on
/// `indexmap`. `neo-p2p` does not depend on `indexmap`, so insertion order is
/// preserved with a [`VecDeque`] + [`HashSet`] pair (both `std`), keeping the
/// same eviction/prune semantics the remote-node dedup path expects.
#[derive(Debug)]
pub struct PendingKnownHashes {
    order: VecDeque<(neo_primitives::UInt256, Instant)>,
    index: HashSet<neo_primitives::UInt256>,
    capacity: usize,
}

impl PendingKnownHashes {
    /// Creates a pending-hash tracker bounded to `capacity` (min 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::with_capacity(capacity.max(1)),
            index: HashSet::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    /// Whether the hash is currently tracked.
    pub fn contains(&self, hash: &neo_primitives::UInt256) -> bool {
        self.index.contains(hash)
    }

    /// Adds `hash` at `timestamp`, pruning stale entries and evicting the oldest
    /// when full. Returns `false` when the hash was already present.
    pub fn try_add(&mut self, hash: neo_primitives::UInt256, timestamp: Instant) -> bool {
        if let Some(cutoff) = timestamp.checked_sub(PENDING_KNOWN_HASH_TTL) {
            self.prune_older_than(cutoff);
        }

        while self.order.len() >= self.capacity && !self.order.is_empty() {
            if let Some((evicted, _)) = self.order.pop_front() {
                self.index.remove(&evicted);
            }
        }

        if self.index.contains(&hash) {
            return false;
        }

        self.index.insert(hash);
        self.order.push_back((hash, timestamp));
        true
    }

    /// Removes `hash`, preserving FIFO order of the remaining entries.
    pub fn remove(&mut self, hash: &neo_primitives::UInt256) -> bool {
        if !self.index.remove(hash) {
            return false;
        }
        if let Some(position) = self.order.iter().position(|(entry, _)| entry == hash) {
            self.order.remove(position);
        }
        true
    }

    /// Drops every tracked hash.
    pub fn clear(&mut self) {
        self.order.clear();
        self.index.clear();
    }

    /// Prunes entries older than `cutoff` from the FIFO front; returns the count.
    pub fn prune_older_than(&mut self, cutoff: Instant) -> usize {
        let mut removed = 0;
        while let Some((hash, timestamp)) = self.order.front() {
            if *timestamp >= cutoff {
                break;
            }
            let hash = *hash;
            self.order.pop_front();
            self.index.remove(&hash);
            removed += 1;
        }
        removed
    }
}

/// Minimal bloom-filter state mirrored from the remote-node filter handlers.
///
/// Only tracks load/add/clear so the routing table stays faithful; producing a
/// `MerkleBlock` reply is a wire-adapter concern (there is no merkle-block
/// payload in `neo-p2p`), so filtered serving is deferred and the session serves
/// full blocks. Documented as a task-89 alignment point.
#[derive(Debug, Default, Clone)]
pub struct BloomFilterState {
    loaded: bool,
    filter: Option<FilterLoadPayload>,
    added: Vec<FilterAddPayload>,
}

impl BloomFilterState {
    /// Whether a filter is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Loads a filter.
    pub fn load(&mut self, payload: FilterLoadPayload) {
        self.loaded = true;
        self.filter = Some(payload);
        self.added.clear();
    }

    /// Adds a pattern to the loaded filter.
    pub fn add(&mut self, payload: FilterAddPayload) {
        if self.loaded {
            self.added.push(payload);
        }
    }

    /// Clears the filter state.
    pub fn clear(&mut self) {
        self.loaded = false;
        self.filter = None;
        self.added.clear();
    }
}

/// Normalizes a signed protocol request count against a maximum.
///
/// Mirrors `RemoteNode::normalize_request`: a negative count means "as many as
/// allowed", otherwise the count is clamped to `max`.
pub fn normalize_request(count: i16, max: usize) -> usize {
    if count < 0 {
        max
    } else {
        (count as usize).min(max)
    }
}

/// Monotonic counter used to keep "budget exceeded" warnings rate-limited.
#[derive(Debug, Default)]
pub struct DropCounter(AtomicU64);

impl DropCounter {
    /// Creates a zeroed counter.
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Increments and returns the new value.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }
}

/// Shared shutdown flag helper (used by reader-task teardown seams).
pub type SharedFlag = Arc<AtomicBool>;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt256;

    fn hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn pending_known_hashes_dedups_and_prunes() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        assert!(cache.try_add(hash(1), now - Duration::from_secs(90)));
        assert!(cache.try_add(hash(2), now));
        // Stale entry (older than TTL) is pruned on the next add.
        assert!(cache.try_add(hash(3), now));
        assert!(!cache.contains(&hash(1)));
        assert!(cache.contains(&hash(2)));
        // Duplicate add is rejected.
        assert!(!cache.try_add(hash(2), now));
    }

    #[test]
    fn pending_known_hashes_evicts_oldest_when_full() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(2);

        assert!(cache.try_add(hash(1), now));
        assert!(cache.try_add(hash(2), now));
        assert!(cache.try_add(hash(3), now));

        assert!(!cache.contains(&hash(1)));
        assert!(cache.contains(&hash(2)));
        assert!(cache.contains(&hash(3)));
    }

    #[test]
    fn normalize_request_matches_protocol_semantics() {
        assert_eq!(normalize_request(-1, 2000), 2000);
        assert_eq!(normalize_request(2500, 2000), 2000);
        assert_eq!(normalize_request(512, 2000), 512);
        assert_eq!(normalize_request(0, 2000), 0);
    }

    #[test]
    fn inbound_command_discriminators_are_stable() {
        use crate::MessageCommand;
        assert_eq!(SessionInbound::Verack.command(), MessageCommand::Verack);
        assert_eq!(SessionInbound::Mempool.command(), MessageCommand::Mempool);
        assert_eq!(SessionOutbound::GetAddr.command(), MessageCommand::GetAddr);
    }
}

//! `TaskManager` sync-scheduling actor body (dest-only migration from `neo-core`).
//!
//! Faithful re-expression of `neo-core::network::p2p::TaskManager` (and its
//! `state` / `scheduling` / `request_flow` / `completion_flow` / `restart_flow`
//! / `timeout_pruning` / `session_lifecycle` / `peer_commands` /
//! `block_validation` / `handle` submodules) on top of the task-105
//! [`TaskManagerPorts`](crate::network::p2p::ports::TaskManagerPorts) bundle.
//!
//! The two structural changes from the original are mandated by the ports
//! boundary:
//!
//! * Sessions are keyed by the opaque [`PeerHandle`] instead of an `ActorRef`
//!   path string, so the manager never names the actor runtime.
//! * `attach_system` is **gone**. Its three reads are performed once against the
//!   ports at construction ([`TaskManager::new`]): `mempool.capacity()` sizes the
//!   known-hash dedup cache, `ledger_query.current_index()` seeds the persist
//!   frontier, and `persistence_events.subscribe()` yields the event receiver
//!   the [`TaskManagerActor`] drains. Outbound peer requests go through
//!   [`PeerCommandPort`](crate::network::p2p::ports::PeerCommandPort) instead of `tell`-ing a
//!   peer `ActorRef`.
//!
//! # Task-89 alignment points
//!
//! * [`Actor`] / [`ActorContext`] / [`ScheduleHandle`] are the local placeholder
//!   runtime from [`super`]; task 89 swaps in `neo-system::runtime`.
//! * Relay-invalid block attribution (disconnecting the peer that supplied a bad
//! * block) needs a hash→peer provenance map the neutral
//!   [`RelayOutcome`](crate::network::p2p::ports::RelayOutcome) does not carry; the handler
//!   releases the index task and logs, leaving attribution to the adapter.

use super::{Actor, ActorContext, ActorResult, ScheduleHandle};
use crate::network::p2p::payloads::{InvPayload, VersionPayload};
use crate::network::p2p::ports::{PeerHandle, PersistenceEvent, RelayOutcome, TaskManagerPorts};
use crate::sessions::TaskSession;
use crate::{ContainsTransactionType, InventoryType, VerifyResult};
use async_trait::async_trait;
use neo_io_crate::HashSetCache;
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;
use std::any::Any;
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Scheduling/housekeeping timer interval.
const TIMER_INTERVAL: Duration = Duration::from_secs(5);
/// Per-task completion timeout before a task is pruned and retried.
const TASK_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the persist frontier may stall before pending index tasks restart.
const PERSISTENCE_FRONTIER_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum peers that may concurrently fetch the same hash/index.
const MAX_CONCURRENT_TASKS: u32 = 3;
/// Maximum hashes carried in one inventory message.
const MAX_HASHES_COUNT: usize = crate::network::p2p::payloads::inv_payload::MAX_HASHES_COUNT;
/// Upper bound of a single `GetBlockByIndex` batch.
const MAX_BLOCK_INDEX_BATCH: u32 = MAX_HASHES_COUNT as u32;
/// Default number of block indexes requested per scheduling pass.
const BLOCK_INDEX_REQUEST_BATCH: u32 = 100;
/// Window multiplier applied to the block-index request limit.
const BLOCK_INDEX_WINDOW_MULTIPLIER: u32 = 1;
/// How long a header request may stay outstanding before it can be retried.
const HEADER_REQUEST_RETRY: Duration = Duration::from_secs(30);
/// Sentinel inventory task marking an in-flight header request for a session.
const HEADER_TASK_HASH: UInt256 = UInt256::zero();

// ===========================================================================
// Global concurrency counter (ported from `task_manager::state::TaskCounter`).
// ===========================================================================

/// Counts how many sessions are concurrently fetching a given key.
///
/// Capped at [`MAX_CONCURRENT_TASKS`] so a single hot hash/index is not
/// redundantly requested from every peer.
#[derive(Debug)]
pub struct TaskCounter<K> {
    counts: HashMap<K, u32>,
}

impl<K: Eq + Hash> TaskCounter<K> {
    /// Creates an empty counter with the given capacity hint.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            counts: HashMap::with_capacity(capacity),
        }
    }

    /// Attempts to reserve one more concurrent fetch of `key`.
    ///
    /// Returns `false` when [`MAX_CONCURRENT_TASKS`] fetches are already
    /// outstanding for `key`, leaving the count unchanged.
    pub fn try_increment(&mut self, key: K) -> bool {
        let entry = self.counts.entry(key).or_insert(0);
        if *entry >= MAX_CONCURRENT_TASKS {
            return false;
        }
        *entry += 1;
        true
    }

    /// Releases one concurrent fetch of `key`, removing it at zero.
    pub fn decrement(&mut self, key: &K) {
        if let Some(count) = self.counts.get_mut(key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.counts.remove(key);
            }
        }
    }

    /// Current outstanding fetch count for `key`.
    pub fn count(&self, key: &K) -> u32 {
        self.counts.get(key).copied().unwrap_or(0)
    }

    /// Whether `key` is currently tracked at all.
    pub fn is_tracked(&self, key: &K) -> bool {
        self.counts.contains_key(key)
    }

    /// Whether another concurrent fetch of `key` can be reserved.
    pub fn has_capacity_for(&self, key: &K) -> bool {
        self.count(key) < MAX_CONCURRENT_TASKS
    }

    /// Whether no keys are tracked.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Number of distinct tracked keys.
    pub fn len(&self) -> usize {
        self.counts.len()
    }
}

// ===========================================================================
// Pure planning helpers (ported from `task_manager::scheduling`).
// ===========================================================================

/// A resolved `GetBlockByIndex` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockIndexRequestPlan {
    /// First index to request.
    pub start_height: u32,
    /// Number of consecutive indexes to request.
    pub count: u32,
}

/// A resolved `GetHeaders` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderRequestPlan {
    /// First header index to request.
    pub start_index: u32,
}

/// Highest index the sliding block window will currently accept.
pub fn block_index_window_limit(current: u32) -> u32 {
    current
        .saturating_add(1)
        .saturating_add(MAX_BLOCK_INDEX_BATCH.saturating_mul(BLOCK_INDEX_WINDOW_MULTIPLIER))
}

/// Plans the next contiguous block-index request for a peer.
///
/// Returns `None` when the peer offers no height beyond the local frontier or
/// the whole window is already saturated with concurrent fetches.
pub fn plan_block_index_request(
    current_height: u32,
    peer_height: u32,
    index_tasks: &TaskCounter<u32>,
) -> Option<BlockIndexRequestPlan> {
    if peer_height <= current_height {
        return None;
    }
    let limit = block_index_window_limit(current_height).min(peer_height);
    let mut start: Option<u32> = None;
    let mut count = 0u32;
    let mut index = current_height.saturating_add(1);
    while index <= limit && count < BLOCK_INDEX_REQUEST_BATCH {
        if index_tasks.has_capacity_for(&index) {
            if start.is_none() {
                start = Some(index);
            }
            count += 1;
        } else if start.is_some() {
            // The contiguous reservable run ended.
            break;
        }
        index = index.saturating_add(1);
    }
    start.map(|start_height| BlockIndexRequestPlan {
        start_height,
        count,
    })
}

/// Combines the in-memory header-cache tip with the persisted header height.
pub fn effective_header_height(header_cache_last: Option<u32>, ledger_highest: u32) -> u32 {
    header_cache_last.map_or(ledger_highest, |tip| tip.max(ledger_highest))
}

/// Plans a header request for a peer whose height exceeds our header tip.
pub fn plan_header_request(
    header_height: u32,
    peer_height: u32,
    header_task_inflight: bool,
    peer_allows_retry: bool,
) -> Option<HeaderRequestPlan> {
    if peer_height <= header_height {
        return None;
    }
    if header_task_inflight && !peer_allows_retry {
        return None;
    }
    Some(HeaderRequestPlan {
        start_index: header_height.saturating_add(1),
    })
}

// ===========================================================================
// Persisted-block matching (ported from `task_manager::block_validation`).
// ===========================================================================

/// How a persisted block relates to the expected frontier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedBlockMatch {
    /// The block is exactly the next expected frontier height.
    Frontier,
    /// The block is at or below the already-persisted height (stale).
    Below,
    /// The block is ahead of the frontier (a gap remains).
    Ahead,
    /// The block's hash does not match the hash recorded for its index.
    Mismatch,
}

/// Classifies a persisted block against the current frontier.
pub fn match_persisted_block(
    persisted_index: u32,
    expected_frontier: u32,
    hash_matches: bool,
) -> PersistedBlockMatch {
    if !hash_matches {
        PersistedBlockMatch::Mismatch
    } else if persisted_index < expected_frontier {
        PersistedBlockMatch::Below
    } else if persisted_index == expected_frontier {
        PersistedBlockMatch::Frontier
    } else {
        PersistedBlockMatch::Ahead
    }
}

// ===========================================================================
// TaskManager state.
// ===========================================================================

/// The sync-scheduling state machine.
///
/// Owns the per-peer [`TaskSession`] bookkeeping, the global concurrency
/// counters, the known-hash dedup cache, and the persist frontier. All external
/// interaction flows through [`TaskManagerPorts`].
pub struct TaskManager {
    ports: TaskManagerPorts,
    sessions: HashMap<PeerHandle, TaskSession>,
    known_hashes: HashSetCache<UInt256>,
    /// Announced inventory type per hash, so a `GetData` reply can be grouped
    /// under the correct [`InventoryType`]. [`TaskSession::available_tasks`] is a
    /// flat hash set (no type), so the manager retains the type seen at `Inv`.
    announced_types: HashMap<UInt256, InventoryType>,
    global_inv_tasks: TaskCounter<UInt256>,
    global_index_tasks: TaskCounter<u32>,
    header_cache_last: Option<u32>,
    last_seen_persisted_index: u32,
    last_persisted_at: Instant,
    timer_interval: Duration,
    task_timeout: Duration,
    persistence_frontier_timeout: Duration,
}

impl TaskManager {
    /// Constructs the manager, performing the one-time wiring that replaced
    /// `attach_system`: sizing the dedup cache from the mempool capacity and
    /// seeding the persist frontier from the ledger's current index.
    pub fn new(ports: TaskManagerPorts) -> Self {
        let capacity = ports.mempool.capacity().max(1);
        let current_index = ports.ledger_query.current_index();
        Self {
            ports,
            sessions: HashMap::new(),
            known_hashes: HashSetCache::new(capacity),
            announced_types: HashMap::new(),
            global_inv_tasks: TaskCounter::with_capacity(64),
            global_index_tasks: TaskCounter::with_capacity(64),
            header_cache_last: None,
            last_seen_persisted_index: current_index,
            last_persisted_at: Instant::now(),
            timer_interval: TIMER_INTERVAL,
            task_timeout: TASK_TIMEOUT,
            persistence_frontier_timeout: PERSISTENCE_FRONTIER_TIMEOUT,
        }
    }

    /// Number of registered peer sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// The persist frontier the manager has observed.
    pub fn last_seen_persisted_index(&self) -> u32 {
        self.last_seen_persisted_index
    }

    // -----------------------------------------------------------------------
    // Session lifecycle (ported from `session_lifecycle`).
    // -----------------------------------------------------------------------

    /// Registers a newly handshaked peer so it becomes schedulable.
    pub fn register_peer(&mut self, peer: PeerHandle, version: VersionPayload) {
        let session = TaskSession::new(&version);
        self.sessions.insert(peer, session);
        self.request_tasks();
    }

    /// Updates a peer's advertised height and reschedules.
    pub fn update_peer(&mut self, peer: PeerHandle, last_block_index: u32) {
        if let Some(session) = self.sessions.get_mut(&peer) {
            session.update_last_block_index(last_block_index);
        }
        self.request_tasks();
    }

    /// Drops all state for a disconnected peer, releasing its global tasks.
    pub fn remove_peer(&mut self, peer: PeerHandle) {
        let Some(session) = self.sessions.remove(&peer) else {
            return;
        };
        for hash in session.pending_inventory_hashes() {
            self.global_inv_tasks.decrement(&hash);
        }
        for index in session.pending_indexes() {
            self.global_index_tasks.decrement(&index);
        }
        self.request_tasks();
    }

    // -----------------------------------------------------------------------
    // Inventory task intake (ported from `request_flow`).
    // -----------------------------------------------------------------------

    /// Records inventory a peer announced and triggers a scheduling pass.
    pub fn new_tasks(&mut self, peer: PeerHandle, payload: InvPayload) {
        let inventory_type = payload.inventory_type;
        for hash in &payload.hashes {
            self.known_hashes.try_add(*hash);
            self.announced_types.insert(*hash, inventory_type);
        }
        if let Some(session) = self.sessions.get_mut(&peer) {
            session.add_available_tasks(payload.hashes.into_iter());
        }
        self.request_tasks();
    }

    /// Re-queues inventory attributed to a specific peer for retry.
    pub fn restart_tasks(&mut self, peer: PeerHandle, payload: InvPayload) {
        let inventory_type = payload.inventory_type;
        for hash in &payload.hashes {
            self.known_hashes.remove(hash);
            self.global_inv_tasks.decrement(hash);
            self.announced_types.insert(*hash, inventory_type);
        }
        if let Some(session) = self.sessions.get_mut(&peer) {
            session.add_available_tasks(payload.hashes.into_iter());
        }
        self.request_tasks();
    }

    /// Re-queues inventory across every registered peer.
    pub fn broadcast_restart_tasks(&mut self, payload: InvPayload) {
        let inventory_type = payload.inventory_type;
        let hashes: Vec<UInt256> = payload.hashes.clone();
        for hash in &hashes {
            self.known_hashes.remove(hash);
            self.global_inv_tasks.decrement(hash);
            self.announced_types.insert(*hash, inventory_type);
        }
        for session in self.sessions.values_mut() {
            session.add_available_tasks(hashes.iter().copied());
        }
        self.request_tasks();
    }

    /// Marks a peer-delivered inventory item complete and releases its task.
    pub fn inventory_completed(
        &mut self,
        peer: PeerHandle,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) {
        self.global_inv_tasks.decrement(&hash);
        self.announced_types.remove(&hash);
        if let Some(session) = self.sessions.get_mut(&peer) {
            session.complete_inv_task(&hash);
            if let (Some(block), Some(index)) = (block, block_index) {
                session.reserve_received_block_slot_for(index);
                session.store_received_block(index, block);
            }
        }
    }

    /// Records that a peer delivered a header batch, clearing the sentinel
    /// in-flight marker registered when the header request was issued.
    pub fn headers(&mut self, peer: PeerHandle) {
        if let Some(session) = self.sessions.get_mut(&peer) {
            session.complete_inv_task(&HEADER_TASK_HASH);
        }
    }

    /// Forgets a hash whose processing failed so a later announcement retries.
    pub fn forget_hash(&mut self, hash: UInt256) {
        self.known_hashes.remove(&hash);
        self.global_inv_tasks.decrement(&hash);
        self.announced_types.remove(&hash);
    }

    /// Whether a hash is already known/tracked.
    pub fn is_known_hash(&self, hash: &UInt256) -> bool {
        self.known_hashes.contains(hash)
    }

    // -----------------------------------------------------------------------
    // Scheduling pass (ported from `scheduling` + `peer_commands`).
    // -----------------------------------------------------------------------

    /// Runs one scheduling pass over every registered session.
    pub fn request_tasks(&mut self) {
        let current_index = self.ports.ledger_query.current_index();
        let header_height = effective_header_height(
            self.header_cache_last,
            self.ports.ledger_query.highest_header_index(),
        );
        let peers: Vec<PeerHandle> = self.sessions.keys().copied().collect();

        for peer in peers {
            self.request_tasks_for_peer(peer, current_index, header_height);
        }
    }

    fn request_tasks_for_peer(&mut self, peer: PeerHandle, current_index: u32, header_height: u32) {
        let (peer_height, is_full_node, saturated) = match self.sessions.get(&peer) {
            Some(session) => (
                session.last_block_index,
                session.is_full_node,
                session.has_too_many_tasks(),
            ),
            None => return,
        };

        if saturated {
            return;
        }

        // 1. Header sync: only from full nodes ahead of our header tip, and only
        //    when the per-session header-request guard allows a (re)request.
        if is_full_node {
            let start_index = header_height.saturating_add(1);
            let allow_request = self
                .sessions
                .get(&peer)
                .map(|session| session.should_request_headers(start_index, HEADER_REQUEST_RETRY))
                .unwrap_or(false);
            if allow_request {
                if let Some(plan) = plan_header_request(header_height, peer_height, false, true) {
                    if self
                        .ports
                        .peer_command
                        .send_get_headers(peer, plan.start_index)
                        .is_ok()
                    {
                        if let Some(session) = self.sessions.get_mut(&peer) {
                            session.record_header_request(plan.start_index);
                            // Sentinel in-flight marker completed by `headers()`.
                            session.register_inv_task(HEADER_TASK_HASH);
                        }
                    }
                }
            }
        }

        // 2. Block-index sync within the sliding window.
        if is_full_node {
            let plan = {
                let counter = &self.global_index_tasks;
                plan_block_index_request(current_index, peer_height, counter)
            };
            if let Some(plan) = plan {
                let mut reserved = Vec::new();
                for index in plan.start_height..plan.start_height.saturating_add(plan.count) {
                    if self.global_index_tasks.try_increment(index) {
                        reserved.push(index);
                    }
                }
                if !reserved.is_empty() {
                    let count = reserved.len() as i16;
                    let start = reserved[0];
                    if let Some(session) = self.sessions.get_mut(&peer) {
                        for index in &reserved {
                            session.register_index_task(*index);
                        }
                    }
                    let _ = self
                        .ports
                        .peer_command
                        .send_get_blocks_by_index(peer, start, count);
                } else {
                    // Nothing could be reserved; release any partial state.
                    for index in &reserved {
                        self.global_index_tasks.decrement(index);
                    }
                }
            }
        }

        // 3. Announced-inventory sync from the session's available pool.
        self.request_available_inventory(peer);

        // 4. Mempool snapshot once the peer is at our frontier.
        let should_send_mempool = self
            .sessions
            .get(&peer)
            .map(|s| !s.mempool_sent && peer_height >= current_index)
            .unwrap_or(false);
        if should_send_mempool {
            if self.ports.peer_command.send_mempool(peer).is_ok() {
                if let Some(session) = self.sessions.get_mut(&peer) {
                    session.mempool_sent = true;
                }
            }
        }
    }

    /// Requests as many available announced hashes as global capacity allows.
    fn request_available_inventory(&mut self, peer: PeerHandle) {
        let candidates: Vec<UInt256> = match self.sessions.get(&peer) {
            Some(session) => session.available_tasks.iter().copied().collect(),
            None => return,
        };

        let mut to_request = Vec::new();
        for hash in candidates {
            if to_request.len() >= MAX_HASHES_COUNT {
                break;
            }
            // Skip items already persisted or in the mempool.
            if self.ports.ledger_query.contains_block(&hash)
                || self.ports.ledger_query.contains_transaction(&hash)
                || self.ports.mempool.contains_transaction(&hash)
                    != ContainsTransactionType::NotExist
            {
                if let Some(session) = self.sessions.get_mut(&peer) {
                    session.available_tasks.remove(&hash);
                }
                continue;
            }
            if self.global_inv_tasks.try_increment(hash) {
                to_request.push(hash);
            }
        }

        if to_request.is_empty() {
            return;
        }

        if let Some(session) = self.sessions.get_mut(&peer) {
            for hash in &to_request {
                session.available_tasks.remove(hash);
                session.register_inv_task(*hash);
            }
        }

        // Group the reserved hashes by their announced inventory type so each
        // `GetData` batch carries the correct discriminator. `available_tasks`
        // is a flat hash set, so the type is taken from `announced_types`
        // (recorded at `Inv` time) and defaults to `Transaction`.
        let mut blocks = Vec::new();
        let mut transactions = Vec::new();
        let mut extensibles = Vec::new();
        for hash in to_request {
            match self
                .announced_types
                .get(&hash)
                .copied()
                .unwrap_or(InventoryType::Transaction)
            {
                InventoryType::Block => blocks.push(hash),
                InventoryType::Extensible | InventoryType::Consensus => extensibles.push(hash),
                InventoryType::Transaction => transactions.push(hash),
            }
        }
        for (inventory_type, group) in [
            (InventoryType::Block, blocks),
            (InventoryType::Transaction, transactions),
            (InventoryType::Extensible, extensibles),
        ] {
            if !group.is_empty() {
                let _ = self
                    .ports
                    .peer_command
                    .send_get_data(peer, inventory_type, group);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Persistence frontier (ported from `completion_flow`).
    // -----------------------------------------------------------------------

    /// Advances the frontier on a committed block and refills the window.
    pub fn on_persist_completed(&mut self, block: &Block) {
        let index = block.index();
        if index > self.last_seen_persisted_index {
            self.last_seen_persisted_index = index;
        }
        self.last_persisted_at = Instant::now();
        self.header_cache_last = Some(self.header_cache_last.unwrap_or(0).max(index));

        // Release the index task and prune each session's received-block cache.
        self.global_index_tasks.decrement(&index);
        for session in self.sessions.values_mut() {
            session.complete_index_task(index);
            session.retain_received_blocks_after(index);
        }
        self.request_tasks();
    }

    /// Releases a verified-but-uncommitted height so it can be re-requested.
    pub fn on_persist_failed(&mut self, index: u32) {
        self.global_index_tasks.decrement(&index);
        for session in self.sessions.values_mut() {
            session.complete_index_task(index);
            session.remove_received_block(index);
        }
        self.last_persisted_at = Instant::now();
        self.request_tasks();
    }

    /// Reacts to a relay outcome (currently releases invalid-block index tasks).
    pub fn on_relay_result(&mut self, outcome: &RelayOutcome) {
        if outcome.inventory_type == InventoryType::Block && outcome.result == VerifyResult::Invalid
        {
            // Task-89 alignment: no hash→peer provenance, so attribution of the
            // disconnect is left to the adapter. We only release the index task.
            if let Some(index) = outcome.block_index {
                self.global_index_tasks.decrement(&index);
                for session in self.sessions.values_mut() {
                    session.complete_index_task(index);
                }
            }
            self.known_hashes.remove(&outcome.hash);
            self.request_tasks();
        }
    }

    // -----------------------------------------------------------------------
    // Timeout pruning (ported from `timeout_pruning`).
    // -----------------------------------------------------------------------

    /// Prunes timed-out tasks and restarts a stalled persist frontier.
    pub fn prune_timeouts(&mut self) {
        let peers: Vec<PeerHandle> = self.sessions.keys().copied().collect();
        for peer in peers {
            let (expired_inv, expired_index) = match self.sessions.get_mut(&peer) {
                Some(session) => (
                    session.prune_timed_out_inv_tasks(self.task_timeout),
                    session.prune_timed_out_index_tasks(self.task_timeout),
                ),
                None => continue,
            };
            for hash in &expired_inv {
                self.global_inv_tasks.decrement(hash);
                self.known_hashes.remove(hash);
            }
            for index in &expired_index {
                self.global_index_tasks.decrement(index);
            }
            if !expired_inv.is_empty() {
                if let Some(session) = self.sessions.get_mut(&peer) {
                    session.add_available_tasks(expired_inv.into_iter());
                }
            }
        }

        // Restart the frontier if persistence has stalled while tasks pend.
        if self.last_persisted_at.elapsed() >= self.persistence_frontier_timeout {
            let stalled: Vec<u32> = self
                .sessions
                .values()
                .flat_map(|s| s.pending_indexes())
                .collect();
            for index in &stalled {
                self.global_index_tasks.decrement(index);
            }
            for session in self.sessions.values_mut() {
                for index in &stalled {
                    session.complete_index_task(*index);
                }
            }
            self.last_persisted_at = Instant::now();
        }

        self.request_tasks();
    }
}

// ===========================================================================
// Actor wrapper.
// ===========================================================================

/// Commands accepted by the [`TaskManagerActor`].
///
/// Mirrors the [`TaskManagerPort`](crate::network::p2p::ports::TaskManagerPort) command set
/// one-for-one (delivered as mailbox messages instead of port calls), plus the
/// internal [`Timer`](Self::Timer) tick and peer-removal the runtime needs.
/// There is deliberately **no** `AttachSystem` variant: wiring happens once in
/// [`TaskManager::new`].
#[derive(Debug, Clone)]
pub enum TaskManagerCommand {
    /// Register a newly handshaked peer.
    Register {
        /// The peer handle.
        peer: PeerHandle,
        /// The peer's advertised version.
        version: VersionPayload,
    },
    /// Update a peer's advertised height.
    Update {
        /// The peer handle.
        peer: PeerHandle,
        /// New last block index.
        last_block_index: u32,
    },
    /// Announced inventory from a peer.
    NewTasks {
        /// The peer handle.
        peer: PeerHandle,
        /// The announced inventory.
        payload: InvPayload,
    },
    /// Restart inventory tasks attributed to a peer.
    RestartTasks {
        /// The peer handle.
        peer: PeerHandle,
        /// Inventory to re-queue.
        payload: InvPayload,
    },
    /// Broadcast an inventory restart to all peers.
    BroadcastRestartTasks {
        /// Inventory to re-queue.
        payload: InvPayload,
    },
    /// A peer-delivered inventory item completed.
    InventoryCompleted {
        /// The peer handle.
        peer: PeerHandle,
        /// The completed hash.
        hash: UInt256,
        /// The received block, when the item was a block.
        block: Option<Block>,
        /// The block index, when known.
        block_index: Option<u32>,
    },
    /// A peer delivered a header batch.
    Headers {
        /// The peer handle.
        peer: PeerHandle,
    },
    /// Forget a failed hash so it can be retried.
    ForgetHash {
        /// The hash to forget.
        hash: UInt256,
    },
    /// A peer disconnected; drop its scheduling state.
    RemovePeer {
        /// The peer handle.
        peer: PeerHandle,
    },
    /// Internal scheduling/housekeeping tick.
    Timer,
}

/// The [`TaskManager`] wrapped as a placeholder-runtime [`Actor`].
///
/// Owns the scheduling timer and the persistence-event receiver so the manager
/// state stays a plain value the actor drives.
pub struct TaskManagerActor {
    manager: TaskManager,
    timer: Option<ScheduleHandle>,
    events: Option<tokio::sync::mpsc::Receiver<PersistenceEvent>>,
}

impl TaskManagerActor {
    /// Wraps a configured [`TaskManager`].
    pub fn new(manager: TaskManager) -> Self {
        Self {
            manager,
            timer: None,
            events: None,
        }
    }

    /// Immutable access to the inner manager (diagnostics/tests).
    pub fn manager(&self) -> &TaskManager {
        &self.manager
    }

    /// Applies one persistence/relay event to the manager state.
    pub fn apply_event(&mut self, event: PersistenceEvent) {
        match event {
            PersistenceEvent::Persisted(persisted) => {
                self.manager.on_persist_completed(&persisted.block);
            }
            PersistenceEvent::Failed(failure) => {
                self.manager.on_persist_failed(failure.index);
            }
            PersistenceEvent::Relay(outcome) => {
                self.manager.on_relay_result(&outcome);
            }
        }
    }
}

#[async_trait]
impl Actor for TaskManagerActor {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        // One-time event subscription that replaced `attach_system`'s stream
        // registration. The adapter drains this receiver and re-delivers values
        // as boxed `PersistenceEvent` messages, or the runtime polls it.
        if self.events.is_none() {
            self.events = Some(self.manager.ports.persistence_events.subscribe());
        }
        if self.timer.is_none() {
            self.timer = Some(ctx.schedule_repeated(
                self.manager.timer_interval,
                self.manager.timer_interval,
                Box::new(TaskManagerCommand::Timer),
            ));
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        // Persistence/relay events arrive as their own boxed value.
        let message = match message.downcast::<PersistenceEvent>() {
            Ok(event) => {
                self.apply_event(*event);
                return Ok(());
            }
            Err(message) => message,
        };

        let Ok(command) = message.downcast::<TaskManagerCommand>() else {
            return Ok(());
        };
        match *command {
            TaskManagerCommand::Register { peer, version } => {
                self.manager.register_peer(peer, version);
            }
            TaskManagerCommand::Update {
                peer,
                last_block_index,
            } => {
                self.manager.update_peer(peer, last_block_index);
            }
            TaskManagerCommand::NewTasks { peer, payload } => {
                self.manager.new_tasks(peer, payload);
            }
            TaskManagerCommand::RestartTasks { peer, payload } => {
                self.manager.restart_tasks(peer, payload);
            }
            TaskManagerCommand::BroadcastRestartTasks { payload } => {
                self.manager.broadcast_restart_tasks(payload);
            }
            TaskManagerCommand::InventoryCompleted {
                peer,
                hash,
                block,
                block_index,
            } => {
                self.manager
                    .inventory_completed(peer, hash, block, block_index);
            }
            TaskManagerCommand::Headers { peer } => {
                self.manager.headers(peer);
            }
            TaskManagerCommand::ForgetHash { hash } => {
                self.manager.forget_hash(hash);
            }
            TaskManagerCommand::RemovePeer { peer } => {
                self.manager.remove_peer(peer);
            }
            TaskManagerCommand::Timer => {
                self.manager.prune_timeouts();
            }
        }
        let _ = ctx;
        Ok(())
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
        self.events = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn task_counter_caps_concurrent_fetches() {
        let mut counter: TaskCounter<UInt256> = TaskCounter::with_capacity(4);
        let key = hash(1);
        for _ in 0..MAX_CONCURRENT_TASKS {
            assert!(counter.try_increment(key));
        }
        assert!(
            !counter.try_increment(key),
            "cap must reject the fourth fetch"
        );
        assert!(!counter.has_capacity_for(&key));
        assert_eq!(counter.count(&key), MAX_CONCURRENT_TASKS);

        counter.decrement(&key);
        assert!(counter.has_capacity_for(&key));
        for _ in 0..MAX_CONCURRENT_TASKS - 1 {
            counter.decrement(&key);
        }
        assert!(counter.is_empty(), "counter must drop keys at zero");
    }

    #[test]
    fn block_index_window_limit_is_one_plus_batch() {
        assert_eq!(
            block_index_window_limit(0),
            1 + MAX_BLOCK_INDEX_BATCH * BLOCK_INDEX_WINDOW_MULTIPLIER
        );
        assert_eq!(block_index_window_limit(100), 101 + MAX_BLOCK_INDEX_BATCH);
    }

    #[test]
    fn plan_block_index_request_respects_peer_height_and_window() {
        let counter: TaskCounter<u32> = TaskCounter::with_capacity(4);
        // Peer at or below the frontier offers nothing.
        assert_eq!(plan_block_index_request(10, 10, &counter), None);
        assert_eq!(plan_block_index_request(10, 5, &counter), None);

        let plan = plan_block_index_request(10, 10_000, &counter).unwrap();
        assert_eq!(plan.start_height, 11);
        assert_eq!(plan.count, BLOCK_INDEX_REQUEST_BATCH);

        // A short peer caps the count at the available height.
        let plan = plan_block_index_request(10, 13, &counter).unwrap();
        assert_eq!(plan.start_height, 11);
        assert_eq!(plan.count, 3);
    }

    #[test]
    fn plan_header_request_gates_on_inflight() {
        assert_eq!(plan_header_request(10, 10, false, false), None);
        assert_eq!(
            plan_header_request(10, 20, true, false),
            None,
            "inflight without retry must not re-request"
        );
        assert_eq!(
            plan_header_request(10, 20, true, true),
            Some(HeaderRequestPlan { start_index: 11 })
        );
        assert_eq!(
            plan_header_request(10, 20, false, false),
            Some(HeaderRequestPlan { start_index: 11 })
        );
    }

    #[test]
    fn effective_header_height_takes_the_max() {
        assert_eq!(effective_header_height(None, 5), 5);
        assert_eq!(effective_header_height(Some(9), 5), 9);
        assert_eq!(effective_header_height(Some(3), 5), 5);
    }

    #[test]
    fn persisted_block_match_classifies_frontier() {
        assert_eq!(
            match_persisted_block(10, 10, true),
            PersistedBlockMatch::Frontier
        );
        assert_eq!(
            match_persisted_block(9, 10, true),
            PersistedBlockMatch::Below
        );
        assert_eq!(
            match_persisted_block(11, 10, true),
            PersistedBlockMatch::Ahead
        );
        assert_eq!(
            match_persisted_block(10, 10, false),
            PersistedBlockMatch::Mismatch
        );
    }
}

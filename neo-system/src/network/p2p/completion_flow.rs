//! Task-manager inventory-completion and persistence-event flow via typed ports.
//!
//! Mirrors `neo-core::network::p2p::task_manager::completion_flow`. The original
//! was a set of `impl TaskManager` methods that mutated the manager's own session
//! map, disconnected offending peers through `disconnect_peer(&ActorRef, ..)`,
//! and re-ran the request planner through `request_tasks_for_path` /
//! `request_tasks_all`. Two things change under the port model:
//!
//! * Peers are opaque [`PeerHandle`]s keyed in a `HashMap`, and per-peer
//!   bookkeeping reuses the existing [`TaskSession`] (task 103) rather than
//!   re-deriving it. Disconnects go through
//!   [`PeerCommandPort::disconnect`](crate::ports::PeerCommandPort).
//! * The request planner is **not** reimplemented here (it is a separate
//!   scheduling concern). Instead each handler *returns* the set of peers that
//!   need a re-request pass, so the caller (the task-manager actor wired in task
//!   89) can feed them into its own `request_tasks_*` logic. This keeps the
//!   completion flow free of any outbound-request policy while preserving the
//!   original control flow.
//!
//! Logging (`tracing`) is not in this crate's allowed dependency set, so the
//! diagnostic branches of the original are collapsed into the same structured
//! [`crate::block_validation`] outcomes; the caller decides what to log.

use super::block_validation::{
    BlockHashMatch, IncomingBlockOutcome, PersistedBlockMatch, block_matches_hash,
    match_persisted_block, persisted_block_hash, validate_incoming_block,
};
use super::ports::{PeerHandle, RelayOutcome, TaskManagerPorts};
use super::scheduling::{HEADER_TASK_HASH, TaskCounter};
use super::sessions::task_session::TaskSession;
use super::{InventoryType, VerifyResult};
use neo_io_crate::HashSetCache;
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;
use std::collections::HashMap;
use std::time::Instant;

/// Mutable task-manager state driven by the completion/persistence flow.
///
/// Holds only the bookkeeping the flow reads and writes: the per-peer
/// [`TaskSession`] map, the two global concurrency counters, the known-hash
/// dedup cache, and the persistence-frontier markers. All sync-progress limits
/// live inside [`TaskSession`]; all outbound policy lives in the caller.
#[derive(Debug)]
pub struct TaskManagerState {
    /// Per-peer task bookkeeping, keyed by opaque handle.
    pub sessions: HashMap<PeerHandle, TaskSession>,
    /// Global count of peers sharing each outstanding block-index task.
    pub global_index_tasks: TaskCounter<u32>,
    /// Global count of peers sharing each outstanding inventory-hash task.
    pub global_inv_tasks: TaskCounter<UInt256>,
    /// Hashes already completed/known across all peers (bounded LRU set).
    pub known_hashes: HashSetCache<UInt256>,
    /// Highest block index observed as persisted.
    pub last_seen_persisted_index: u32,
    /// When the last successful persistence was observed.
    pub last_persisted_at: Option<Instant>,
}

impl TaskManagerState {
    /// Creates an empty state with a known-hash cache of `known_hash_capacity`.
    pub fn new(known_hash_capacity: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            global_index_tasks: TaskCounter::with_capacity(0),
            global_inv_tasks: TaskCounter::with_capacity(0),
            known_hashes: HashSetCache::new(known_hash_capacity),
            last_seen_persisted_index: 0,
            last_persisted_at: None,
        }
    }

    /// Registers (or replaces) a peer's task session.
    pub fn register_session(&mut self, peer: PeerHandle, session: TaskSession) {
        self.sessions.insert(peer, session);
    }

    /// Drops a peer's task session, releasing its share of every global task.
    pub fn remove_session(&mut self, peer: &PeerHandle) {
        let Some(session) = self.sessions.remove(peer) else {
            return;
        };
        for index in session.index_tasks.keys() {
            self.global_index_tasks.decrement(index);
        }
        for hash in session.inv_tasks.keys() {
            self.global_inv_tasks.decrement(hash);
        }
    }

    /// Removes a hash from every session's globally-available task set.
    fn remove_available_task_from_all(&mut self, hash: &UInt256) {
        for session in self.sessions.values_mut() {
            session.available_tasks.remove(hash);
        }
    }

    /// Records a completed inventory hash in the dedup cache.
    fn record_known_hash(&mut self, hash: UInt256) {
        self.known_hashes.try_add(hash);
    }

    /// Completes a peer-delivered inventory item.
    ///
    /// Releases the peer's inventory/index task and its share of the global
    /// counters, validates and buffers a delivered block, disconnects the peer on
    /// a conflicting/unhashable block, and returns the peers that need a
    /// re-request pass (empty when the peer was disconnected or the received-block
    /// budget is exhausted).
    pub fn complete_inventory(
        &mut self,
        ports: &TaskManagerPorts,
        peer: PeerHandle,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) -> Vec<PeerHandle> {
        if hash != HEADER_TASK_HASH {
            self.record_known_hash(hash);
        }
        self.remove_available_task_from_all(&hash);

        let Some(session) = self.sessions.get_mut(&peer) else {
            return Vec::new();
        };

        // Capture whether this block was actually requested before completing the
        // task entries (which removes them from the session).
        let block_was_requested = block.as_ref().is_some_and(|block_ref| {
            let index = block_ref.index();
            session.index_tasks.contains_key(&index)
                || session.inv_tasks.contains_key(&hash)
                || session.received_block.contains_key(&index)
        });

        if session.complete_inv_task(&hash) {
            self.global_inv_tasks.decrement(&hash);
        }

        let mut index_to_release = block_index;
        if let Some(ref block_ref) = block {
            index_to_release = Some(block_ref.index());
        }
        if let Some(index) = index_to_release {
            if session.complete_index_task(index) {
                self.global_index_tasks.decrement(&index);
            }
        }

        let mut should_request = true;
        let mut disconnect_reason: Option<&'static str> = None;

        if let Some(block_payload) = block {
            // Keep the request pipeline full after each delivered block; persistence
            // may lag block download by many blocks.
            should_request = true;
            let index = block_payload.index();
            let requested = block_was_requested
                || session.index_tasks.contains_key(&index)
                || session.inv_tasks.contains_key(&hash)
                || session.received_block.contains_key(&index);

            if requested {
                let existing = session.received_block.get(&index);
                let outcome = validate_incoming_block(block_payload, existing);
                match outcome {
                    IncomingBlockOutcome::Store { index, block } => {
                        if !session.store_received_block(index, block) {
                            // The index task is already complete. Requesting the same
                            // height while the buffer is still full is what turned one
                            // overflow into millions of repeat drops.
                            should_request = false;
                        }
                    }
                    IncomingBlockOutcome::KeepExisting => {}
                    IncomingBlockOutcome::Disconnect {
                        index,
                        reason,
                        remove_cached,
                        ..
                    } => {
                        if remove_cached {
                            session.remove_received_block(index);
                        }
                        disconnect_reason = Some(reason.reason());
                    }
                }
            }
            // An unsolicited block is never retained: it is the cheapest way for a
            // peer to pin memory before validation. `should_request` stays true.
        }

        if hash == HEADER_TASK_HASH {
            session.available_tasks.remove(&hash);
        }

        if let Some(reason) = disconnect_reason {
            let _ = ports.peer_command.disconnect(peer, reason.to_string());
            return Vec::new();
        }

        if should_request {
            return vec![peer];
        }
        Vec::new()
    }

    /// Handles a successfully persisted block: matches every session's cached
    /// copy at that height, disconnects peers whose cached block disagrees with
    /// the persisted hash, advances the frontier, drops completed index tasks,
    /// and returns all peers for a re-request pass.
    pub fn on_persist_completed(
        &mut self,
        ports: &TaskManagerPorts,
        block: &Block,
    ) -> Vec<PeerHandle> {
        self.last_seen_persisted_index = block.index();
        self.last_persisted_at = Some(Instant::now());
        let index = block.index();

        let hash = match persisted_block_hash(block) {
            Ok(hash) => hash,
            // Unhashable persisted block: skip matching but keep the frontier.
            Err(_) => return self.all_peers(),
        };

        let mut to_disconnect = Vec::new();
        for (peer, session) in self.sessions.iter_mut() {
            if let Some(stored) = session.remove_received_block(index) {
                match match_persisted_block(stored, &hash) {
                    PersistedBlockMatch::Matches => {}
                    PersistedBlockMatch::Mismatch => to_disconnect.push(*peer),
                    PersistedBlockMatch::Unhashable(_) => to_disconnect.push(*peer),
                }
            }
        }

        for peer in &to_disconnect {
            let _ =
                ports
                    .peer_command
                    .disconnect(*peer, "persisted block hash mismatch".to_string());
        }

        self.global_index_tasks.retain_tracked(|idx| *idx > index);
        for session in self.sessions.values_mut() {
            session.index_tasks.retain(|&idx, _| idx > index);
        }

        self.all_peers()
    }

    /// Handles a local commit failure for a verified block: releases the cached
    /// payload and completed index task at that height so it can be requested
    /// again, then returns all peers for a re-request pass.
    pub fn on_persist_failed(&mut self, index: u32) -> Vec<PeerHandle> {
        for session in self.sessions.values_mut() {
            session.remove_received_block(index);
            if session.complete_index_task(index) {
                self.global_index_tasks.decrement(&index);
            }
        }
        self.all_peers()
    }

    /// Handles a relay outcome, disconnecting offenders only for an invalid block.
    pub fn on_relay_result(
        &mut self,
        ports: &TaskManagerPorts,
        result: &RelayOutcome,
    ) -> Vec<PeerHandle> {
        if result.result != VerifyResult::Invalid || result.inventory_type != InventoryType::Block {
            return Vec::new();
        }
        self.on_invalid_block(ports, &result.hash, result.block_index)
    }

    /// Disconnects every peer whose cached block matches an invalid block hash and
    /// returns all peers for a re-request pass.
    pub fn on_invalid_block(
        &mut self,
        ports: &TaskManagerPorts,
        hash: &UInt256,
        block_index: Option<u32>,
    ) -> Vec<PeerHandle> {
        let mut offenders = Vec::new();

        for (peer, session) in self.sessions.iter() {
            let mut matches = false;
            if let Some(index) = block_index {
                if let Some(stored) = session.received_block.get(&index) {
                    matches = match block_matches_hash(stored, hash) {
                        BlockHashMatch::Matches => true,
                        BlockHashMatch::DoesNotMatch => false,
                        // An unhashable cached block is treated as an offender.
                        BlockHashMatch::Unhashable(_) => true,
                    };
                }
            } else {
                for stored in session.received_block.values() {
                    match block_matches_hash(stored, hash) {
                        BlockHashMatch::Matches => {
                            matches = true;
                            break;
                        }
                        BlockHashMatch::DoesNotMatch => {}
                        BlockHashMatch::Unhashable(_) => {
                            matches = true;
                            break;
                        }
                    }
                }
            }

            if matches {
                offenders.push(*peer);
            }
        }

        for peer in &offenders {
            let _ = ports
                .peer_command
                .disconnect(*peer, "invalid block relayed".to_string());
        }

        self.all_peers()
    }

    /// All currently registered peer handles.
    fn all_peers(&self) -> Vec<PeerHandle> {
        self.sessions.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::PeerHandle;
    use neo_protocol::payloads::Block;
    use std::sync::{Arc, Mutex};

    /// Records disconnects and returns every peer for re-request assertions.
    #[derive(Default)]
    struct RecordingPeerCommand {
        disconnected: Mutex<Vec<(u64, String)>>,
    }

    impl crate::ports::PeerCommandPort for RecordingPeerCommand {
        fn send_get_data(
            &self,
            _peer: PeerHandle,
            _inventory_type: InventoryType,
            _hashes: Vec<UInt256>,
        ) -> crate::ports::PortResult<()> {
            Ok(())
        }
        fn send_get_headers(&self, _peer: PeerHandle, _start_index: u32) -> crate::ports::PortResult<()> {
            Ok(())
        }
        fn send_get_blocks_by_index(
            &self,
            _peer: PeerHandle,
            _start_index: u32,
            _count: i16,
        ) -> crate::ports::PortResult<()> {
            Ok(())
        }
        fn send_mempool(&self, _peer: PeerHandle) -> crate::ports::PortResult<()> {
            Ok(())
        }
        fn disconnect(&self, peer: PeerHandle, reason: String) -> crate::ports::PortResult<()> {
            self.disconnected.lock().unwrap().push((peer.as_u64(), reason));
            Ok(())
        }
    }

    fn block_at(index: u32, nonce: u64) -> Block {
        let mut block = Block::new();
        block.header.set_index(index);
        block.header.set_nonce(nonce);
        block
    }

    struct Harness {
        ports: TaskManagerPorts,
        peer_command: Arc<RecordingPeerCommand>,
    }

    fn harness() -> Harness {
        let peer_command = Arc::new(RecordingPeerCommand::default());
        let ports = TaskManagerPorts {
            ledger_query: Arc::new(StubLedger),
            mempool: Arc::new(StubMempool),
            peer_command: peer_command.clone(),
            persistence_events: Arc::new(StubEvents),
        };
        Harness { ports, peer_command }
    }

    #[test]
    fn complete_inventory_releases_task_and_requests_again() {
        let h = harness();
        let mut state = TaskManagerState::new(64);
        let peer = PeerHandle::new(1);

        let mut session = TaskSession::default();
        let hash = UInt256::from([7u8; 32]);
        session.register_inv_task(hash);
        state.register_session(peer, session);
        state.global_inv_tasks.try_increment(hash);

        let to_request = state.complete_inventory(&h.ports, peer, hash, None, None);

        assert_eq!(to_request, vec![peer]);
        assert!(!state.global_inv_tasks.is_tracked(&hash));
        assert!(state.known_hashes.contains(&hash));
    }

    #[test]
    fn complete_inventory_disconnects_on_conflicting_block() {
        let h = harness();
        let mut state = TaskManagerState::new(64);
        let peer = PeerHandle::new(2);

        let mut session = TaskSession::default();
        // Cache a block at height 5 with a different nonce than the incoming one.
        session.store_received_block(5, block_at(5, 1));
        state.register_session(peer, session);

        let incoming = block_at(5, 2);
        let mut hash_holder = incoming.clone();
        let hash = hash_holder.try_hash().expect("hash");

        let to_request =
            state.complete_inventory(&h.ports, peer, hash, Some(incoming), Some(5));

        assert!(to_request.is_empty(), "a disconnected peer is not re-requested");
        let disconnected = h.peer_command.disconnected.lock().unwrap();
        assert_eq!(disconnected.len(), 1);
        assert_eq!(disconnected[0].0, 2);
        assert_eq!(disconnected[0].1, "conflicting block received");
    }

    #[test]
    fn persist_completed_advances_frontier_and_drops_index_tasks() {
        let h = harness();
        let mut state = TaskManagerState::new(64);
        let peer = PeerHandle::new(3);

        let persisted = block_at(9, 1);
        let mut session = TaskSession::default();
        session.register_index_task(9);
        session.store_received_block(9, persisted.clone());
        state.register_session(peer, session);
        state.global_index_tasks.try_increment(9);

        let to_request = state.on_persist_completed(&h.ports, &persisted);

        assert_eq!(to_request, vec![peer]);
        assert_eq!(state.last_seen_persisted_index, 9);
        assert!(state.last_persisted_at.is_some());
        assert!(!state.global_index_tasks.is_tracked(&9));
    }

    #[test]
    fn relay_result_only_reacts_to_invalid_blocks() {
        let h = harness();
        let mut state = TaskManagerState::new(64);
        let peer = PeerHandle::new(4);

        let bad = block_at(3, 1);
        let mut hash_holder = bad.clone();
        let hash = hash_holder.try_hash().expect("hash");

        let mut session = TaskSession::default();
        session.store_received_block(3, bad);
        state.register_session(peer, session);

        let invalid_block = RelayOutcome {
            inventory_type: InventoryType::Block,
            hash,
            block_index: Some(3),
            result: VerifyResult::Invalid,
        };
        let to_request = state.on_relay_result(&h.ports, &invalid_block);
        assert_eq!(to_request, vec![peer]);
        assert_eq!(h.peer_command.disconnected.lock().unwrap().len(), 1);

        // A successful relay is ignored entirely.
        let ok_relay = RelayOutcome {
            inventory_type: InventoryType::Block,
            hash,
            block_index: Some(3),
            result: VerifyResult::Succeed,
        };
        assert!(state.on_relay_result(&h.ports, &ok_relay).is_empty());
    }

    // --- Stub ports (only peer_command is exercised by the completion flow) ---

    struct StubLedger;
    impl crate::ports::LedgerQueryPort for StubLedger {
        fn current_index(&self) -> u32 {
            0
        }
        fn highest_header_index(&self) -> u32 {
            0
        }
        fn contains_block(&self, _hash: &UInt256) -> bool {
            false
        }
        fn contains_transaction(&self, _hash: &UInt256) -> bool {
            false
        }
        fn block_hash_at(&self, _index: u32) -> Option<UInt256> {
            None
        }
        fn block_hashes_from(&self, _start: &UInt256, _count: usize) -> Vec<UInt256> {
            Vec::new()
        }
        fn try_get_block(&self, _hash: &UInt256) -> Option<Block> {
            None
        }
    }

    struct StubMempool;
    impl crate::ports::MempoolQueryPort for StubMempool {
        fn capacity(&self) -> usize {
            0
        }
        fn transaction_hashes(&self) -> Vec<UInt256> {
            Vec::new()
        }
        fn try_get_transaction(&self, _hash: &UInt256) -> Option<neo_protocol::payloads::Transaction> {
            None
        }
        fn contains_transaction(&self, _hash: &UInt256) -> crate::ContainsTransactionType {
            crate::ContainsTransactionType::NotExist
        }
        fn contains_conflict_hash(&self, _hash: &UInt256, _signers: &[neo_primitives::UInt160]) -> bool {
            false
        }
        fn try_get_relay_extensible(&self, _hash: &UInt256) -> Option<neo_protocol::payloads::Extensible> {
            None
        }
        fn try_get_extensible(&self, _hash: &UInt256) -> Option<neo_protocol::payloads::Extensible> {
            None
        }
    }

    struct StubEvents;
    impl crate::ports::PersistenceEventPort for StubEvents {
        fn subscribe(&self) -> tokio::sync::mpsc::Receiver<crate::ports::PersistenceEvent> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }
}

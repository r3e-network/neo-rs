//! Session state mirrored from `Neo.Network.P2P.TaskSession`.
//!
//! Each remote peer tracked by the task manager maintains an independent view
//! of which inventories or indexes are currently being requested.  The C#
//! implementation stores this information inside `TaskSession`.  The
//! translation below keeps the same fields and behaviour so that the ported
//! `TaskManager` can rely on identical semantics.

use crate::UInt256;
use crate::neo_io::Serializable;
use crate::network::p2p::{
    capabilities::NodeCapability,
    payloads::{VersionPayload, block::Block},
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct HeaderRequestState {
    start_index: u32,
    requested_at: Instant,
}

/// Mirrors the per-peer bookkeeping performed by the C# task manager.
#[derive(Debug, Default)]
pub struct TaskSession {
    /// Inventory hashes currently requested from the peer.
    pub inv_tasks: HashMap<UInt256, Instant>,
    /// Header/block indexes currently requested from the peer.
    pub index_tasks: HashMap<u32, Instant>,
    /// Hashes that are available globally but owned by another session.
    pub available_tasks: HashSet<UInt256>,
    /// Blocks received but not yet persisted. This holds full block payloads while
    /// waiting for validation/persistence to complete.
    pub received_block: HashMap<u32, Block>,
    /// Approximate serialized bytes currently held in [`Self::received_block`].
    received_block_bytes: usize,
    /// Whether the remote peer advertised the full-node capability.
    pub is_full_node: bool,
    /// Highest block index reported by the peer.
    pub last_block_index: u32,
    /// Whether the peer has already been sent the mempool snapshot.
    pub mempool_sent: bool,
    /// Tracks the last header range request issued to this peer.
    last_header_request: Option<HeaderRequestState>,
}

impl TaskSession {
    /// Upper bound on outstanding tasks. The C# reference uses 100; a larger
    /// value keeps the pipelined range requests effective while still bounding
    /// per-peer amplification (the previous value was 50,000).
    pub const MAX_PENDING_TASKS: usize = 4096;

    /// Maximum serialized bytes of unverified blocks retained for a peer.
    ///
    /// Blocks are cached here *before* blockchain validation, so this budget is
    /// the primary defence against a peer pinning memory. It is intentionally
    /// lower than the full 1000-block request window: persistence advances the
    /// window and refills it.
    pub const MAX_RECEIVED_BLOCK_BYTES: usize = 64 * 1024 * 1024;

    /// Maximum number of unverified blocks retained for a peer.
    pub const MAX_RECEIVED_BLOCKS: usize = 256;

    /// Creates a new session for the supplied peer version payload.
    pub fn new(version: &VersionPayload) -> Self {
        let mut is_full_node = false;
        let mut last_block_index = 0u32;

        for capability in &version.capabilities {
            if let NodeCapability::FullNode { start_height } = capability {
                is_full_node = true;
                last_block_index = *start_height;
                break;
            }
        }

        Self {
            inv_tasks: HashMap::with_capacity(64),
            index_tasks: HashMap::with_capacity(64),
            available_tasks: HashSet::with_capacity(64),
            received_block: HashMap::with_capacity(64),
            received_block_bytes: 0,
            is_full_node,
            last_block_index,
            mempool_sent: false,
            last_header_request: None,
        }
    }

    /// Updates the remote peer's advertised block height.
    pub fn update_last_block_index(&mut self, last_block_index: u32) {
        self.last_block_index = last_block_index;
    }

    /// Returns `true` if the session is already saturated with work.
    pub fn has_too_many_tasks(&self) -> bool {
        self.inv_tasks.len() + self.index_tasks.len() >= Self::MAX_PENDING_TASKS
    }

    /// Records an inventory hash currently in-flight.
    pub fn register_inv_task(&mut self, hash: UInt256) {
        self.inv_tasks.insert(hash, Instant::now());
    }

    /// Records an index currently being retrieved.
    pub fn register_index_task(&mut self, index: u32) {
        self.index_tasks.insert(index, Instant::now());
    }

    /// Marks an inventory task as completed.
    pub fn complete_inv_task(&mut self, hash: &UInt256) -> bool {
        self.inv_tasks.remove(hash).is_some()
    }

    /// Marks an index task as completed.
    pub fn complete_index_task(&mut self, index: u32) -> bool {
        self.index_tasks.remove(&index).is_some()
    }

    /// Adds hashes that are available with other peers.
    pub fn add_available_tasks<I>(&mut self, hashes: I)
    where
        I: IntoIterator<Item = UInt256>,
    {
        self.available_tasks.extend(hashes);
    }

    /// Stores a received block for later validation.
    ///
    /// Returns `false` when the per-peer byte/block budget would be exceeded;
    /// the caller must not treat such a block as queued for persistence.
    pub fn store_received_block(&mut self, index: u32, block: Block) -> bool {
        let size = block.size();
        let replaced = self.received_block.remove(&index);
        if let Some(old) = &replaced {
            self.received_block_bytes = self.received_block_bytes.saturating_sub(old.size());
        }

        let over_budget = self.received_block.len() >= Self::MAX_RECEIVED_BLOCKS
            || self.received_block_bytes.saturating_add(size) > Self::MAX_RECEIVED_BLOCK_BYTES;

        if over_budget {
            if let Some(old) = replaced {
                self.received_block.insert(index, old);
                self.received_block_bytes = self.received_block_bytes.saturating_add(size);
            }
            return false;
        }

        self.received_block.insert(index, block);
        self.received_block_bytes = self.received_block_bytes.saturating_add(size);
        true
    }

    /// Removes a cached received block and updates the byte accounting.
    pub fn remove_received_block(&mut self, index: u32) -> Option<Block> {
        let block = self.received_block.remove(&index)?;
        self.received_block_bytes = self.received_block_bytes.saturating_sub(block.size());
        Some(block)
    }

    /// Retains only received blocks with an index greater than `persisted`.
    pub fn retain_received_blocks_after(&mut self, persisted: u32) {
        self.received_block.retain(|&idx, _| idx > persisted);
        self.received_block_bytes = self
            .received_block
            .values()
            .map(Block::size)
            .fold(0usize, usize::saturating_add);
    }

    /// Returns the approximate serialized bytes held in `received_block`.
    #[must_use]
    pub fn received_block_bytes(&self) -> usize {
        self.received_block_bytes
    }

    /// Returns true if a header request should be issued for the supplied start index.
    ///
    /// This prevents flooding peers with the exact same `GetHeaders` range while
    /// the blockchain actor is still processing previously received headers.
    pub fn should_request_headers(&self, start_index: u32, retry_after: Duration) -> bool {
        match self.last_header_request {
            None => true,
            Some(state) => {
                state.start_index != start_index || state.requested_at.elapsed() >= retry_after
            }
        }
    }

    /// Records the most recent header request.
    pub fn record_header_request(&mut self, start_index: u32) {
        self.last_header_request = Some(HeaderRequestState {
            start_index,
            requested_at: Instant::now(),
        });
    }

    /// Removes and returns any inventory tasks that exceeded the timeout.
    pub fn prune_timed_out_inv_tasks(&mut self, timeout: Duration) -> Vec<UInt256> {
        Self::prune_tasks(&mut self.inv_tasks, timeout)
    }

    /// Removes and returns any index tasks that exceeded the timeout.
    pub fn prune_timed_out_index_tasks(&mut self, timeout: Duration) -> Vec<u32> {
        self.prune_index_tasks(timeout)
    }

    fn prune_tasks<T>(tasks: &mut HashMap<T, Instant>, timeout: Duration) -> Vec<T>
    where
        T: Eq + std::hash::Hash + Copy,
    {
        let now = Instant::now();
        let mut expired = Vec::new();
        tasks.retain(|key, started_at| {
            if now.duration_since(*started_at) >= timeout {
                expired.push(*key);
                false
            } else {
                true
            }
        });
        expired
    }

    fn prune_index_tasks(&mut self, timeout: Duration) -> Vec<u32> {
        Self::prune_tasks(&mut self.index_tasks, timeout)
    }

    /// Returns an iterator of hashes currently requested from the peer.
    pub fn pending_inventory_hashes(&self) -> impl Iterator<Item = UInt256> + '_ {
        self.inv_tasks.keys().copied()
    }

    /// Returns an iterator of indexes currently requested from the peer.
    pub fn pending_indexes(&self) -> impl Iterator<Item = u32> + '_ {
        self.index_tasks.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_at(index: u32) -> Block {
        let mut block = Block::new();
        block.header.set_index(index);
        block
    }

    #[test]
    fn received_block_budget_is_enforced_and_released() {
        let mut session = TaskSession::new(&VersionPayload::default());

        for index in 0..TaskSession::MAX_RECEIVED_BLOCKS as u32 {
            assert!(session.store_received_block(index, block_at(index)));
        }
        assert_eq!(
            session.received_block.len(),
            TaskSession::MAX_RECEIVED_BLOCKS
        );
        assert!(
            !session.store_received_block(
                TaskSession::MAX_RECEIVED_BLOCKS as u32,
                block_at(TaskSession::MAX_RECEIVED_BLOCKS as u32)
            ),
            "a full per-peer cache must reject new blocks"
        );

        let bytes_before = session.received_block_bytes();
        assert!(session.remove_received_block(0).is_some());
        assert!(session.received_block_bytes() < bytes_before);
        assert!(
            session.store_received_block(
                TaskSession::MAX_RECEIVED_BLOCKS as u32,
                block_at(TaskSession::MAX_RECEIVED_BLOCKS as u32)
            ),
            "space freed by persistence/removal must be reusable"
        );
    }
}

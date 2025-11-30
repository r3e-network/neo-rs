//! Session state mirrored from `Neo.Network.P2P.TaskSession`.
//!
//! Each remote peer tracked by the task manager maintains an independent view
//! of which inventories or indexes are currently being requested.  The C#
//! implementation stores this information inside `TaskSession`.  The
//! translation below keeps the same fields and behaviour so that the ported
//! `TaskManager` can rely on identical semantics.

use crate::network::p2p::{
    capabilities::NodeCapability,
    payloads::{block::Block, VersionPayload},
};
use crate::UInt256;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
    /// Whether the remote peer advertised the full-node capability.
    pub is_full_node: bool,
    /// Highest block index reported by the peer.
    pub last_block_index: u32,
    /// Whether the peer has already been sent the mempool snapshot.
    pub mempool_sent: bool,
}

impl TaskSession {
    /// Upper bound on outstanding tasks (mirror of the C# constant).
    pub const MAX_PENDING_TASKS: usize = 100;

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
            inv_tasks: HashMap::new(),
            index_tasks: HashMap::new(),
            available_tasks: HashSet::new(),
            received_block: HashMap::new(),
            is_full_node,
            last_block_index,
            mempool_sent: false,
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
    pub fn store_received_block(&mut self, index: u32, block: Block) {
        self.received_block.insert(index, block);
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

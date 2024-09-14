use std::cmp::Ordering;
use std::hash::Hash;
use std::time::{SystemTime, UNIX_EPOCH};
use NeoRust::builder::Transaction;
use NeoRust::neo_protocol::HighPriorityAttribute;

/// Represents an item in the Memory Pool.
///
/// Note: PoolItem objects don't consider transaction priority (low or high) in their compare
/// CompareTo method. This is because items of differing priority are never added to the same
/// sorted set in MemoryPool.
pub struct PoolItem {
    /// Internal transaction for PoolItem
    pub tx: Transaction,

    /// Timestamp when transaction was stored on PoolItem
    pub timestamp: u64,

    /// Timestamp when this transaction was last broadcast to other nodes
    pub last_broadcast_timestamp: u64,
}

impl PoolItem {
    pub fn new(tx: Transaction) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Self {
            tx,
            timestamp: now,
            last_broadcast_timestamp: now,
        }
    }

    fn compare_to(&self, other_tx: &Transaction) -> Ordering {
        let self_high_priority = self.tx.get_attribute::<HighPriorityAttribute>().is_some();
        let other_high_priority = other_tx.get_attribute::<HighPriorityAttribute>().is_some();

        if self_high_priority != other_high_priority {
            return self_high_priority.cmp(&other_high_priority);
        }

        // Fees sorted ascending
        match self.tx.fee_per_byte().cmp(&other_tx.fee_per_byte()) {
            Ordering::Equal => {}
            ord => return ord,
        }

        match self.tx.network_fee().cmp(&other_tx.network_fee()) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Transaction hash sorted descending
        other_tx.hash().cmp(&self.tx.hash())
    }
}

impl PartialEq for PoolItem {
    fn eq(&self, other: &Self) -> bool {
        self.tx.hash() == other.tx.hash()
    }
}

impl Eq for PoolItem {}

impl PartialOrd for PoolItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PoolItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(&other.tx)
    }
}

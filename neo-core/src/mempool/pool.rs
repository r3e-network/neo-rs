//! Transaction mempool implementation

use super::{FeePolicy, MempoolError, MempoolResult, TransactionEntry, DEFAULT_MAX_TRANSACTIONS};
use hashbrown::{HashMap, HashSet};
use neo_primitives::{UInt160, UInt256};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;

/// Mempool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolConfig {
    /// Maximum number of transactions
    pub max_transactions: usize,

    /// Maximum transactions per sender
    pub max_per_sender: usize,

    /// Fee policy
    #[serde(default)]
    pub fee_policy: FeePolicy,

    /// Enable transaction replacement (RBF)
    pub enable_replacement: bool,

    /// Minimum fee increase for replacement (percentage)
    pub replacement_fee_increase: u8,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_transactions: DEFAULT_MAX_TRANSACTIONS,
            max_per_sender: 100,
            fee_policy: FeePolicy::default(),
            enable_replacement: true,
            replacement_fee_increase: 10, // 10% minimum increase
        }
    }
}

/// Interior state protected by a single lock.
struct MempoolInner {
    transactions: HashMap<UInt256, TransactionEntry>,
    by_sender: HashMap<UInt160, HashSet<UInt256>>,
    priority_queue: BinaryHeap<PriorityEntry>,
    current_height: u32,
}

/// Transaction mempool
pub struct Mempool {
    /// Configuration
    config: MempoolConfig,

    /// All mutable state behind a single RwLock to prevent multi-lock deadlocks.
    inner: RwLock<MempoolInner>,
}

/// Entry for the priority queue
#[derive(Debug, Clone, Eq, PartialEq)]
struct PriorityEntry {
    hash: UInt256,
    priority: i64,
    fee_per_byte: i64,
}

impl Ord for PriorityEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.fee_per_byte.cmp(&other.fee_per_byte))
    }
}

impl PartialOrd for PriorityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Mempool {
    /// Create a new mempool with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MempoolConfig::default())
    }

    /// Create a new mempool with custom configuration
    #[must_use]
    pub fn with_config(config: MempoolConfig) -> Self {
        Self {
            config,
            inner: RwLock::new(MempoolInner {
                transactions: HashMap::new(),
                by_sender: HashMap::new(),
                priority_queue: BinaryHeap::new(),
                current_height: 0,
            }),
        }
    }

    /// Add a transaction to the pool
    pub fn add(&self, entry: TransactionEntry) -> MempoolResult<()> {
        let hash = entry.hash;

        // Validate under read lock first
        {
            let inner = self.inner.read();

            if inner.transactions.contains_key(&hash) {
                return Err(MempoolError::DuplicateTransaction(hash));
            }

            if !self
                .config
                .fee_policy
                .is_fee_acceptable(entry.network_fee, entry.size)
            {
                return Err(MempoolError::InsufficientFee {
                    required: self.config.fee_policy.minimum_fee(entry.size),
                    actual: entry.network_fee,
                });
            }

            if entry.is_expired(inner.current_height) {
                return Err(MempoolError::Expired(entry.valid_until_block));
            }

            if let Some(sender_txs) = inner.by_sender.get(&entry.sender) {
                if sender_txs.len() >= self.config.max_per_sender {
                    return Err(MempoolError::TooManyFromSender(self.config.max_per_sender));
                }
            }
        }

        // Acquire write lock for mutation
        let mut inner = self.inner.write();

        // Check capacity — may need to evict
        if inner.transactions.len() >= self.config.max_transactions {
            if !Self::try_evict_lowest_inner(&mut inner, &entry) {
                return Err(MempoolError::PoolFull(self.config.max_transactions));
            }
        }

        // Re-check duplicate under write lock (another thread may have inserted)
        if inner.transactions.contains_key(&hash) {
            return Err(MempoolError::DuplicateTransaction(hash));
        }

        inner.transactions.insert(hash, entry.clone());
        inner.by_sender.entry(entry.sender).or_default().insert(hash);
        inner.priority_queue.push(PriorityEntry {
            hash,
            priority: entry.priority,
            fee_per_byte: entry.fee_per_byte(),
        });

        tracing::debug!("Added transaction to mempool: {:?}", hash);
        Ok(())
    }

    /// Remove a transaction from the pool
    pub fn remove(&self, hash: &UInt256) -> Option<TransactionEntry> {
        let mut inner = self.inner.write();
        Self::remove_inner(&mut inner, hash)
    }

    /// Remove helper that operates on the already-locked inner state.
    fn remove_inner(inner: &mut MempoolInner, hash: &UInt256) -> Option<TransactionEntry> {
        if let Some(entry) = inner.transactions.remove(hash) {
            if let Some(sender_txs) = inner.by_sender.get_mut(&entry.sender) {
                sender_txs.remove(hash);
                if sender_txs.is_empty() {
                    inner.by_sender.remove(&entry.sender);
                }
            }
            // Note: We don't remove from priority_queue immediately for performance.
            // Stale entries are filtered when dequeuing.
            tracing::debug!("Removed transaction from mempool: {:?}", hash);
            return Some(entry);
        }
        None
    }

    /// Get a transaction by hash
    pub fn get(&self, hash: &UInt256) -> Option<TransactionEntry> {
        self.inner.read().transactions.get(hash).cloned()
    }

    /// Check if a transaction exists
    pub fn contains(&self, hash: &UInt256) -> bool {
        self.inner.read().transactions.contains_key(hash)
    }

    /// Get current pool size
    pub fn len(&self) -> usize {
        self.inner.read().transactions.len()
    }

    /// Check if pool is empty
    pub fn is_empty(&self) -> bool {
        self.inner.read().transactions.is_empty()
    }

    /// Get all transaction hashes
    pub fn hashes(&self) -> Vec<UInt256> {
        self.inner.read().transactions.keys().copied().collect()
    }

    /// Get top N transactions by priority
    pub fn get_top(&self, n: usize) -> Vec<TransactionEntry> {
        let inner = self.inner.read();
        let mut entries: Vec<_> = inner.transactions.values().cloned().collect();
        entries.sort();
        entries.truncate(n);
        entries
    }

    /// Get transactions for a sender
    pub fn get_by_sender(&self, sender: &UInt160) -> Vec<TransactionEntry> {
        let inner = self.inner.read();
        inner
            .by_sender
            .get(sender)
            .map(|hashes| {
                hashes
                    .iter()
                    .filter_map(|h| inner.transactions.get(h).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Update the current block height and remove expired transactions
    pub fn update_height(&self, height: u32) {
        {
            self.inner.write().current_height = height;
        }
        self.remove_expired();
        self.update_fee_policy();
    }

    /// Remove expired transactions
    pub fn remove_expired(&self) -> usize {
        let mut inner = self.inner.write();
        let current_height = inner.current_height;

        let expired: Vec<UInt256> = inner
            .transactions
            .iter()
            .filter(|(_, entry)| entry.is_expired(current_height))
            .map(|(hash, _)| *hash)
            .collect();

        let count = expired.len();
        for hash in expired {
            Self::remove_inner(&mut inner, &hash);
        }

        if count > 0 {
            tracing::info!("Removed {} expired transactions", count);
        }

        count
    }

    /// Clear all transactions
    pub fn clear(&self) {
        let mut inner = self.inner.write();
        inner.transactions.clear();
        inner.by_sender.clear();
        inner.priority_queue.clear();
    }

    /// Try to evict lowest priority transaction to make room (operates on locked inner).
    fn try_evict_lowest_inner(inner: &mut MempoolInner, new_entry: &TransactionEntry) -> bool {
        let lowest = inner.transactions.values().max_by(std::cmp::Ord::cmp);

        if let Some(lowest) = lowest {
            if new_entry.priority > lowest.priority {
                let hash = lowest.hash;
                Self::remove_inner(inner, &hash);
                return true;
            }
        }

        false
    }

    /// Update fee policy based on pool utilization
    fn update_fee_policy(&self) {
        let len = self.inner.read().transactions.len();
        let utilization = len as f64 / self.config.max_transactions as f64;

        // Update fee policy based on current pool congestion level
        let mut config = self.config.clone();
        config.fee_policy.update_congestion(utilization);
    }

    /// Get pool statistics
    pub fn stats(&self) -> MempoolStats {
        let inner = self.inner.read();

        let total_fees: i64 = inner
            .transactions
            .values()
            .map(super::transaction_entry::TransactionEntry::total_fee)
            .sum();
        let total_size: usize = inner.transactions.values().map(|e| e.size).sum();

        MempoolStats {
            transaction_count: inner.transactions.len(),
            sender_count: inner.by_sender.len(),
            total_fees,
            total_size,
            capacity: self.config.max_transactions,
            utilization: inner.transactions.len() as f64 / self.config.max_transactions as f64,
        }
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

/// Mempool statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolStats {
    /// Number of transactions
    pub transaction_count: usize,

    /// Number of unique senders
    pub sender_count: usize,

    /// Total fees (datoshi)
    pub total_fees: i64,

    /// Total size (bytes)
    pub total_size: usize,

    /// Maximum capacity
    pub capacity: usize,

    /// Utilization (0.0 to 1.0)
    pub utilization: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TransactionEntryParams;

    fn create_test_entry(hash_byte: u8, priority: i64) -> TransactionEntry {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0] = hash_byte;

        TransactionEntry::new(TransactionEntryParams {
            hash: UInt256::from(hash_bytes),
            sender: UInt160::zero(),
            system_fee: 1_000_000,
            network_fee: 10_000_000,
            size: 100,
            valid_until_block: 10000,
            priority,
            data: vec![0u8; 100],
        })
    }

    #[test]
    fn test_add_and_get() {
        let pool = Mempool::new();
        let entry = create_test_entry(1, 100);
        let hash = entry.hash;

        pool.add(entry).unwrap();

        assert!(pool.contains(&hash));
        assert_eq!(pool.len(), 1);

        let retrieved = pool.get(&hash).unwrap();
        assert_eq!(retrieved.hash, hash);
    }

    #[test]
    fn test_duplicate_rejection() {
        let pool = Mempool::new();
        let entry = create_test_entry(1, 100);

        pool.add(entry.clone()).unwrap();
        let result = pool.add(entry);

        assert!(matches!(result, Err(MempoolError::DuplicateTransaction(_))));
    }

    #[test]
    fn test_remove() {
        let pool = Mempool::new();
        let entry = create_test_entry(1, 100);
        let hash = entry.hash;

        pool.add(entry).unwrap();
        let removed = pool.remove(&hash);

        assert!(removed.is_some());
        assert!(!pool.contains(&hash));
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_expiration() {
        let pool = Mempool::new();
        let entry = create_test_entry(1, 100);

        pool.add(entry).unwrap();
        assert_eq!(pool.len(), 1);

        pool.update_height(10001);
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_stats() {
        let pool = Mempool::new();

        for i in 0..10 {
            pool.add(create_test_entry(i, 100 - i as i64)).unwrap();
        }

        let stats = pool.stats();
        assert_eq!(stats.transaction_count, 10);
        assert!(stats.utilization > 0.0);
    }
}

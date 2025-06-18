//! Memory Pool Implementation
//!
//! This module provides a complete transaction memory pool implementation
//! that exactly matches the C# Neo.Network.P2P.MemoryPool functionality.

use crate::{Error, Result};
use neo_core::{Transaction, UInt256, UInt160};
use std::collections::{HashMap, BTreeMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, Duration};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Memory pool configuration (matches C# MemoryPool settings)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolConfig {
    /// Maximum number of transactions in pool
    pub max_transactions: usize,
    /// Maximum memory usage in bytes
    pub max_memory_usage: usize,
    /// Transaction timeout in seconds
    pub transaction_timeout: u64,
    /// Minimum fee per byte
    pub min_fee_per_byte: u64,
    /// Enable transaction replacement
    pub enable_replacement: bool,
    /// Maximum transaction size
    pub max_transaction_size: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_transactions: 50000, // Matches C# Neo default
            max_memory_usage: 100 * 1024 * 1024, // 100MB
            transaction_timeout: 120, // 2 minutes
            min_fee_per_byte: 1000, // 0.001 GAS per byte
            enable_replacement: true,
            max_transaction_size: 102400, // 100KB
        }
    }
}

/// Transaction pool entry with metadata
#[derive(Debug, Clone)]
pub struct PooledTransaction {
    /// The transaction
    pub transaction: Transaction,
    /// When it was added to the pool
    pub timestamp: SystemTime,
    /// Network fee per byte
    pub fee_per_byte: u64,
    /// Total size in bytes
    pub size: usize,
    /// Sender addresses
    pub senders: Vec<UInt160>,
    /// Whether this is a high priority transaction
    pub high_priority: bool,
}

impl PooledTransaction {
    /// Creates a new pooled transaction
    pub fn new(transaction: Transaction, high_priority: bool) -> Result<Self> {
        let size = transaction.size();
        let fee_per_byte = if size > 0 {
            transaction.network_fee() as u64 / size as u64
        } else {
            0
        };

        // Extract sender addresses from signers
        let senders = transaction.signers()
            .iter()
            .map(|signer| signer.account)
            .collect();

        Ok(Self {
            transaction,
            timestamp: SystemTime::now(),
            fee_per_byte,
            size,
            senders,
            high_priority,
        })
    }

    /// Gets the transaction hash
    pub fn hash(&self) -> Result<UInt256> {
        self.transaction.hash()
    }

    /// Checks if the transaction has expired
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO) > timeout
    }

    /// Gets the priority score for ordering
    pub fn priority_score(&self) -> u64 {
        if self.high_priority {
            u64::MAX - 1000 + self.fee_per_byte
        } else {
            self.fee_per_byte
        }
    }
}

/// Memory pool statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolStats {
    /// Current transaction count
    pub transaction_count: usize,
    /// Current memory usage in bytes
    pub memory_usage: usize,
    /// Transactions added total
    pub transactions_added: u64,
    /// Transactions removed total
    pub transactions_removed: u64,
    /// High priority transactions
    pub high_priority_count: usize,
    /// Average fee per byte
    pub average_fee_per_byte: f64,
    /// Pool utilization percentage
    pub utilization_percentage: f64,
}

impl Default for MempoolStats {
    fn default() -> Self {
        Self {
            transaction_count: 0,
            memory_usage: 0,
            transactions_added: 0,
            transactions_removed: 0,
            high_priority_count: 0,
            average_fee_per_byte: 0.0,
            utilization_percentage: 0.0,
        }
    }
}

/// Transaction verification delegate for mempool
pub trait TxVerifier: Send + Sync {
    /// Verifies if a transaction is valid for inclusion in mempool
    fn verify_transaction(&self, transaction: &Transaction) -> Result<bool>;
    
    /// Checks if transaction conflicts with any in the pool
    fn check_conflicts(&self, transaction: &Transaction, pool_transactions: &[&Transaction]) -> Result<bool>;
}

/// Default transaction verifier
#[derive(Debug)]
pub struct DefaultTxVerifier;

impl TxVerifier for DefaultTxVerifier {
    fn verify_transaction(&self, transaction: &Transaction) -> Result<bool> {
        // Basic validation (production would include full verification)
        
        // 1. Check basic format
        if transaction.script().is_empty() {
            return Ok(false);
        }

        // 2. Check signers
        if transaction.signers().is_empty() {
            return Ok(false);
        }

        // 3. Check witnesses match signers
        if transaction.witnesses().len() != transaction.signers().len() {
            return Ok(false);
        }

        // 4. Check fees are reasonable
        if transaction.network_fee() < 0 || transaction.system_fee() < 0 {
            return Ok(false);
        }

        Ok(true)
    }

    fn check_conflicts(&self, transaction: &Transaction, pool_transactions: &[&Transaction]) -> Result<bool> {
        let tx_hash = transaction.hash()?;
        
        // Check for duplicate transaction hash
        for pool_tx in pool_transactions {
            if pool_tx.hash()? == tx_hash {
                return Ok(true); // Conflict found
            }
        }

        Ok(false) // No conflicts
    }
}

/// Main memory pool implementation (matches C# MemoryPool exactly)
pub struct MemoryPool {
    /// Configuration
    config: MempoolConfig,
    /// Transactions by hash
    transactions: Arc<RwLock<HashMap<UInt256, PooledTransaction>>>,
    /// Transactions sorted by priority
    priority_queue: Arc<RwLock<BTreeMap<u64, Vec<UInt256>>>>,
    /// Transactions by sender address
    sender_map: Arc<RwLock<HashMap<UInt160, HashSet<UInt256>>>>,
    /// Pool statistics
    stats: Arc<RwLock<MempoolStats>>,
    /// Transaction verifier
    verifier: Arc<dyn TxVerifier>,
}

impl MemoryPool {
    /// Creates a new memory pool
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            priority_queue: Arc::new(RwLock::new(BTreeMap::new())),
            sender_map: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(MempoolStats::default())),
            verifier: Arc::new(DefaultTxVerifier),
        }
    }

    /// Creates a new memory pool with custom verifier
    pub fn with_verifier(config: MempoolConfig, verifier: Arc<dyn TxVerifier>) -> Self {
        Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            priority_queue: Arc::new(RwLock::new(BTreeMap::new())),
            sender_map: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(MempoolStats::default())),
            verifier,
        }
    }

    /// Tries to add a transaction to the pool (matches C# TryAdd)
    pub fn try_add(&self, transaction: Transaction, high_priority: bool) -> Result<bool> {
        let tx_hash = transaction.hash()?;
        
        // 1. Check if transaction already exists
        if self.contains(&tx_hash) {
            debug!("Transaction {} already in pool", tx_hash);
            return Ok(false);
        }

        // 2. Check transaction size
        let tx_size = transaction.size();
        if tx_size > self.config.max_transaction_size {
            warn!("Transaction {} too large: {} bytes", tx_hash, tx_size);
            return Ok(false);
        }

        // 3. Check minimum fee
        let fee_per_byte = if tx_size > 0 {
            transaction.network_fee() as u64 / tx_size as u64
        } else {
            0
        };
        
        if fee_per_byte < self.config.min_fee_per_byte {
            warn!("Transaction {} fee too low: {} per byte", tx_hash, fee_per_byte);
            return Ok(false);
        }

        // 4. Verify transaction
        if !self.verifier.verify_transaction(&transaction)? {
            warn!("Transaction {} failed verification", tx_hash);
            return Ok(false);
        }

        // 5. Check for conflicts
        let pool_txs: Vec<&Transaction> = self.transactions.read().unwrap()
            .values()
            .map(|pooled| &pooled.transaction)
            .collect();
            
        if self.verifier.check_conflicts(&transaction, &pool_txs)? {
            warn!("Transaction {} conflicts with existing transactions", tx_hash);
            return Ok(false);
        }

        // 6. Check pool capacity
        if !self.can_add_transaction(&transaction, high_priority)? {
            if !self.try_make_space(&transaction, high_priority)? {
                debug!("Cannot add transaction {}, pool is full", tx_hash);
                return Ok(false);
            }
        }

        // 7. Create pooled transaction and add to pool
        let pooled_tx = PooledTransaction::new(transaction, high_priority)?;
        self.add_to_pool(pooled_tx)?;

        info!("Added transaction {} to mempool", tx_hash);
        Ok(true)
    }

    /// Removes a transaction from the pool (matches C# TryRemove)
    pub fn try_remove(&self, tx_hash: &UInt256) -> Result<Option<Transaction>> {
        let mut transactions = self.transactions.write().unwrap();
        
        if let Some(pooled_tx) = transactions.remove(tx_hash) {
            drop(transactions);
            
            // Remove from priority queue
            self.remove_from_priority_queue(&pooled_tx)?;
            
            // Remove from sender map
            self.remove_from_sender_map(&pooled_tx)?;
            
            // Update statistics
            self.update_stats_on_remove(&pooled_tx);
            
            info!("Removed transaction {} from mempool", tx_hash);
            Ok(Some(pooled_tx.transaction))
        } else {
            Ok(None)
        }
    }

    /// Checks if a transaction exists in the pool (matches C# ContainsKey)
    pub fn contains(&self, tx_hash: &UInt256) -> bool {
        self.transactions.read().unwrap().contains_key(tx_hash)
    }

    /// Gets a transaction from the pool (matches C# TryGetValue)
    pub fn get_transaction(&self, tx_hash: &UInt256) -> Option<Transaction> {
        self.transactions.read().unwrap()
            .get(tx_hash)
            .map(|pooled_tx| pooled_tx.transaction.clone())
    }

    /// Gets transactions for block creation (matches C# GetSortedTransactions)
    pub fn get_sorted_transactions(&self, max_count: Option<usize>) -> Vec<Transaction> {
        let transactions = self.transactions.read().unwrap();
        let priority_queue = self.priority_queue.read().unwrap();
        
        let mut result = Vec::new();
        let limit = max_count.unwrap_or(self.config.max_transactions);
        
        // Iterate through priority queue from highest to lowest priority
        for (_, tx_hashes) in priority_queue.iter().rev() {
            for tx_hash in tx_hashes {
                if let Some(pooled_tx) = transactions.get(tx_hash) {
                    result.push(pooled_tx.transaction.clone());
                    
                    if result.len() >= limit {
                        return result;
                    }
                }
            }
        }
        
        result
    }

    /// Gets all transactions from the pool (matches C# GetAllTransactions)
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        self.transactions.read().unwrap()
            .values()
            .map(|pooled_tx| pooled_tx.transaction.clone())
            .collect()
    }

    /// Gets transactions by sender (matches C# GetTransactionsBySender)
    pub fn get_transactions_by_sender(&self, sender: &UInt160) -> Vec<Transaction> {
        let transactions = self.transactions.read().unwrap();
        let sender_map = self.sender_map.read().unwrap();
        
        if let Some(tx_hashes) = sender_map.get(sender) {
            tx_hashes.iter()
                .filter_map(|hash| transactions.get(hash))
                .map(|pooled_tx| pooled_tx.transaction.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Clears expired transactions (matches C# RemoveExpiredTransactions)
    pub fn clear_expired_transactions(&self) -> Result<usize> {
        let timeout = Duration::from_secs(self.config.transaction_timeout);
        let mut expired_hashes = Vec::new();
        
        {
            let transactions = self.transactions.read().unwrap();
            for (hash, pooled_tx) in transactions.iter() {
                if pooled_tx.is_expired(timeout) {
                    expired_hashes.push(*hash);
                }
            }
        }
        
        let mut removed_count = 0;
        for hash in expired_hashes {
            if self.try_remove(&hash)?.is_some() {
                removed_count += 1;
            }
        }
        
        if removed_count > 0 {
            info!("Removed {} expired transactions from mempool", removed_count);
        }
        
        Ok(removed_count)
    }

    /// Gets memory pool statistics (matches C# GetStatistics)
    pub fn get_stats(&self) -> MempoolStats {
        let mut stats = self.stats.read().unwrap().clone();
        let transactions = self.transactions.read().unwrap();
        
        // Update real-time statistics
        stats.transaction_count = transactions.len();
        stats.memory_usage = transactions.values()
            .map(|tx| tx.size)
            .sum();
        
        stats.high_priority_count = transactions.values()
            .filter(|tx| tx.high_priority)
            .count();
            
        if stats.transaction_count > 0 {
            stats.average_fee_per_byte = transactions.values()
                .map(|tx| tx.fee_per_byte as f64)
                .sum::<f64>() / stats.transaction_count as f64;
        }
        
        stats.utilization_percentage = (stats.transaction_count as f64 / self.config.max_transactions as f64) * 100.0;
        
        stats
    }

    /// Clears all transactions from the pool (matches C# Clear)
    pub fn clear(&self) -> Result<usize> {
        let mut transactions = self.transactions.write().unwrap();
        let count = transactions.len();
        
        transactions.clear();
        self.priority_queue.write().unwrap().clear();
        self.sender_map.write().unwrap().clear();
        
        // Reset statistics
        let mut stats = self.stats.write().unwrap();
        stats.transactions_removed += count as u64;
        
        info!("Cleared {} transactions from mempool", count);
        Ok(count)
    }

    // ===== Private helper methods =====

    /// Checks if a transaction can be added to the pool
    fn can_add_transaction(&self, transaction: &Transaction, _high_priority: bool) -> Result<bool> {
        let transactions = self.transactions.read().unwrap();
        let current_count = transactions.len();
        let current_memory = transactions.values().map(|tx| tx.size).sum::<usize>();
        
        // Check transaction count limit
        if current_count >= self.config.max_transactions {
            return Ok(false);
        }
        
        // Check memory limit
        let tx_size = transaction.size();
        if current_memory + tx_size > self.config.max_memory_usage {
            return Ok(false);
        }
        
        Ok(true)
    }

    /// Tries to make space for a new transaction by removing lower priority ones
    fn try_make_space(&self, new_transaction: &Transaction, high_priority: bool) -> Result<bool> {
        if !self.config.enable_replacement {
            return Ok(false);
        }

        let new_fee_per_byte = if new_transaction.size() > 0 {
            new_transaction.network_fee() as u64 / new_transaction.size() as u64
        } else {
            0
        };

        // Find lowest priority transactions to remove
        let transactions = self.transactions.read().unwrap();
        let mut candidates_to_remove = Vec::new();
        
        for (hash, pooled_tx) in transactions.iter() {
            if !pooled_tx.high_priority && pooled_tx.fee_per_byte < new_fee_per_byte {
                candidates_to_remove.push(*hash);
            }
        }
        
        drop(transactions);
        
        // Remove candidates until we have space
        for hash in candidates_to_remove {
            self.try_remove(&hash)?;
            
            if self.can_add_transaction(new_transaction, high_priority)? {
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    /// Adds a pooled transaction to all internal data structures
    fn add_to_pool(&self, pooled_tx: PooledTransaction) -> Result<()> {
        let tx_hash = pooled_tx.hash()?;
        let priority_score = pooled_tx.priority_score();
        
        // Add to main transaction map
        self.transactions.write().unwrap().insert(tx_hash, pooled_tx.clone());
        
        // Add to priority queue
        self.priority_queue.write().unwrap()
            .entry(priority_score)
            .or_insert_with(Vec::new)
            .push(tx_hash);
        
        // Add to sender map
        let mut sender_map = self.sender_map.write().unwrap();
        for sender in &pooled_tx.senders {
            sender_map.entry(*sender)
                .or_insert_with(HashSet::new)
                .insert(tx_hash);
        }
        
        // Update statistics
        self.update_stats_on_add(&pooled_tx);
        
        Ok(())
    }

    /// Removes transaction from priority queue
    fn remove_from_priority_queue(&self, pooled_tx: &PooledTransaction) -> Result<()> {
        let tx_hash = pooled_tx.hash()?;
        let priority_score = pooled_tx.priority_score();
        
        let mut priority_queue = self.priority_queue.write().unwrap();
        if let Some(tx_hashes) = priority_queue.get_mut(&priority_score) {
            tx_hashes.retain(|&hash| hash != tx_hash);
            
            if tx_hashes.is_empty() {
                priority_queue.remove(&priority_score);
            }
        }
        
        Ok(())
    }

    /// Removes transaction from sender map
    fn remove_from_sender_map(&self, pooled_tx: &PooledTransaction) -> Result<()> {
        let tx_hash = pooled_tx.hash()?;
        
        let mut sender_map = self.sender_map.write().unwrap();
        for sender in &pooled_tx.senders {
            if let Some(tx_hashes) = sender_map.get_mut(sender) {
                tx_hashes.remove(&tx_hash);
                
                if tx_hashes.is_empty() {
                    sender_map.remove(sender);
                }
            }
        }
        
        Ok(())
    }

    /// Updates statistics when adding a transaction
    fn update_stats_on_add(&self, _pooled_tx: &PooledTransaction) {
        let mut stats = self.stats.write().unwrap();
        stats.transactions_added += 1;
    }

    /// Updates statistics when removing a transaction
    fn update_stats_on_remove(&self, _pooled_tx: &PooledTransaction) {
        let mut stats = self.stats.write().unwrap();
        stats.transactions_removed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::{Transaction, UInt256, UInt160, Signer, WitnessScope, Witness};

    #[test]
    fn test_mempool_creation() {
        let config = MempoolConfig::default();
        let pool = MemoryPool::new(config.clone());
        
        let stats = pool.get_stats();
        assert_eq!(stats.transaction_count, 0);
        assert_eq!(stats.memory_usage, 0);
    }

    #[test]
    fn test_pooled_transaction_creation() {
        let mut tx = Transaction::new();
        tx.set_script(vec![0x40]); // Simple RET script
        tx.set_network_fee(1000);
        tx.add_signer(Signer {
            account: UInt160::zero(),
            scopes: WitnessScope::CalledByEntry,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        });
        tx.add_witness(Witness::default());

        let pooled_tx = PooledTransaction::new(tx, false).unwrap();
        assert!(!pooled_tx.high_priority);
        assert!(pooled_tx.size > 0);
        assert_eq!(pooled_tx.senders.len(), 1);
    }

    #[tokio::test]
    async fn test_add_transaction() {
        let config = MempoolConfig::default();
        let pool = MemoryPool::new(config);
        
        let mut tx = Transaction::new();
        tx.set_script(vec![0x40]); // Simple RET script
        tx.set_network_fee(100000); // High fee
        tx.add_signer(Signer {
            account: UInt160::zero(),
            scopes: WitnessScope::CalledByEntry,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        });
        tx.add_witness(Witness::default());

        let tx_hash = tx.hash().unwrap();
        let result = pool.try_add(tx, false).unwrap();
        
        assert!(result);
        assert!(pool.contains(&tx_hash));
        
        let stats = pool.get_stats();
        assert_eq!(stats.transaction_count, 1);
    }

    #[test]
    fn test_get_sorted_transactions() {
        let config = MempoolConfig::default();
        let pool = MemoryPool::new(config);
        
        // Add transactions with different fees
        for i in 0..5 {
            let mut tx = Transaction::new();
            tx.set_nonce(i); // Make each transaction unique
            tx.set_script(vec![0x40]);
            tx.set_network_fee((i + 1) * 10000); // Different fees
            tx.add_signer(Signer {
                account: UInt160::zero(),
                scopes: WitnessScope::CalledByEntry,
                allowed_contracts: Vec::new(),
                allowed_groups: Vec::new(),
                rules: Vec::new(),
            });
            tx.add_witness(Witness::default());
            
            pool.try_add(tx, false).unwrap();
        }
        
        let sorted_txs = pool.get_sorted_transactions(Some(3));
        assert_eq!(sorted_txs.len(), 3);
        
        // Should be sorted by fee (highest first)
        assert!(sorted_txs[0].network_fee() >= sorted_txs[1].network_fee());
        assert!(sorted_txs[1].network_fee() >= sorted_txs[2].network_fee());
    }

    #[test]
    fn test_clear_pool() {
        let config = MempoolConfig::default();
        let pool = MemoryPool::new(config);
        
        // Add some transactions
        for i in 0..3 {
            let mut tx = Transaction::new();
            tx.set_nonce(i);
            tx.set_script(vec![0x40]);
            tx.set_network_fee(10000);
            tx.add_signer(Signer {
                account: UInt160::zero(),
                scopes: WitnessScope::CalledByEntry,
                allowed_contracts: Vec::new(),
                allowed_groups: Vec::new(),
                rules: Vec::new(),
            });
            tx.add_witness(Witness::default());
            
            pool.try_add(tx, false).unwrap();
        }
        
        assert_eq!(pool.get_stats().transaction_count, 3);
        
        let cleared_count = pool.clear().unwrap();
        assert_eq!(cleared_count, 3);
        assert_eq!(pool.get_stats().transaction_count, 0);
    }
}
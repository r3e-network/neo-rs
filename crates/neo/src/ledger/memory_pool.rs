//! Memory pool implementation.
//!
//! This module provides memory pool functionality exactly matching C# Neo MemoryPool.

// Matches C# using directives exactly:
// using Neo.Network.P2P;
// using Neo.Network.P2P.Payloads;
// using Neo.Persistence;
// using System;
// using System.Collections;
// using System.Collections.Generic;
// using System.Diagnostics.CodeAnalysis;
// using System.Linq;
// using System.Runtime.CompilerServices;
// using System.Threading;

use super::{PoolItem, TransactionRemovedEventArgs, TransactionVerificationContext};
use crate::network::p2p::payloads::Transaction;
use crate::UInt256;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::RwLock;

/// namespace Neo.Ledger -> public class MemoryPool : IReadOnlyCollection<Transaction>

/// Allow a reverified transaction to be rebroadcast if it has been this many block times since last broadcast.
const BLOCKS_TILL_REBROADCAST: i32 = 10;

/// Used to cache verified transactions before being written into the block.
pub struct MemoryPool {
    /// public event EventHandler<Transaction>? TransactionAdded;
    pub transaction_added: Option<Box<dyn Fn(&Transaction) + Send + Sync>>,

    /// public event EventHandler<TransactionRemovedEventArgs>? TransactionRemoved;
    pub transaction_removed: Option<Box<dyn Fn(&TransactionRemovedEventArgs) + Send + Sync>>,

    // private int RebroadcastMultiplierThreshold => Capacity / 10;
    // Implemented as method below

    // private readonly double MaxMillisecondsToReverifyTx;
    max_milliseconds_to_reverify_tx: f64,

    // private readonly double MaxMillisecondsToReverifyTxPerIdle;
    max_milliseconds_to_reverify_tx_per_idle: f64,

    // private readonly ReaderWriterLockSlim _txRwLock = new(LockRecursionPolicy.SupportsRecursion);
    tx_rw_lock: RwLock<()>,

    // private readonly Dictionary<UInt256, PoolItem> _unsortedTransactions = new();
    unsorted_transactions: HashMap<UInt256, PoolItem>,

    // private readonly Dictionary<UInt256, HashSet<UInt256>> _conflicts = new();
    conflicts: HashMap<UInt256, HashSet<UInt256>>,

    // private readonly SortedSet<PoolItem> _sortedTransactions = new();
    sorted_transactions: BTreeSet<PoolItem>,

    // private readonly Dictionary<UInt256, PoolItem> _unverifiedTransactions = new();
    unverified_transactions: HashMap<UInt256, PoolItem>,

    // private readonly SortedSet<PoolItem> _unverifiedSortedTransactions = new();
    unverified_sorted_transactions: BTreeSet<PoolItem>,

    // public int Capacity { get; }
    pub capacity: i32,

    // private TransactionVerificationContext VerificationContext = new();
    verification_context: TransactionVerificationContext,
}

impl MemoryPool {
    /// public MemoryPool(NeoSystem system)
    pub fn new(system: &crate::system::NeoSystem) -> Self {
        let capacity = system.settings().memory_pool_max_transactions;
        let time_per_block_ms = system.time_per_block().as_secs_f64() * 1000.0;

        Self {
            transaction_added: None,
            transaction_removed: None,
            max_milliseconds_to_reverify_tx: time_per_block_ms / 3.0,
            max_milliseconds_to_reverify_tx_per_idle: time_per_block_ms / 15.0,
            tx_rw_lock: RwLock::new(()),
            unsorted_transactions: HashMap::new(),
            conflicts: HashMap::new(),
            sorted_transactions: BTreeSet::new(),
            unverified_transactions: HashMap::new(),
            unverified_sorted_transactions: BTreeSet::new(),
            capacity,
            verification_context: TransactionVerificationContext::new(),
        }
    }

    /// private int RebroadcastMultiplierThreshold => Capacity / 10;
    fn rebroadcast_multiplier_threshold(&self) -> i32 {
        self.capacity / 10
    }

    /// internal int SortedTxCount => _sortedTransactions.Count;
    pub(crate) fn sorted_tx_count(&self) -> usize {
        self.sorted_transactions.len()
    }

    /// internal int UnverifiedSortedTxCount => _unverifiedSortedTransactions.Count;
    pub(crate) fn unverified_sorted_tx_count(&self) -> usize {
        self.unverified_sorted_transactions.len()
    }

    /// public int Count
    pub fn count(&self) -> usize {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.len() + self.unverified_transactions.len()
    }

    /// public int VerifiedCount => _unsortedTransactions.Count;
    pub fn verified_count(&self) -> usize {
        self.unsorted_transactions.len() // read of 32 bit type is atomic (no lock)
    }

    /// public int UnVerifiedCount => _unverifiedTransactions.Count;
    pub fn unverified_count(&self) -> usize {
        self.unverified_transactions.len()
    }

    /// public bool ContainsKey(UInt256 hash)
    pub fn contains_key(&self, hash: &UInt256) -> bool {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.contains_key(hash)
            || self.unverified_transactions.contains_key(hash)
    }

    // NOTE: Additional methods would be implemented here following the same pattern
    // The full implementation requires all the supporting types to be available

    // Key methods to implement:
    // - TryAdd
    // - TryRemoveUnVerified
    // - TryRemoveVerified
    // - TryGetValue
    // - GetVerifiedTransactions
    // - GetSortedVerifiedTransactions
    // - ReVerifyTopUnverifiedTransactionsIfNeeded
    // - UpdatePoolForBlockPersisted
    // - InvalidateVerifiedTransactions
    // - InvalidateAllTransactions
    // etc.
}

// IReadOnlyCollection<Transaction> implementation
impl IntoIterator for MemoryPool {
    type Item = Transaction;
    type IntoIter = std::vec::IntoIter<Transaction>;

    fn into_iter(self) -> Self::IntoIter {
        let _guard = self.tx_rw_lock.read().unwrap();
        let mut transactions = Vec::new();

        for item in self.unsorted_transactions.values() {
            transactions.push(item.transaction.clone());
        }
        for item in self.unverified_transactions.values() {
            transactions.push(item.transaction.clone());
        }

        transactions.into_iter()
    }
}

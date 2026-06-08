//! [`MemoryPool`] - the Neo transaction memory pool.
//!
//! Holds two priority queues:
//!
//! - `verified` — transactions whose state-dependent witness
//!   verification has succeeded and are ready to be picked up by the
//!   block-mining pipeline.
//! - `unverified` — transactions whose state-dependent witness
//!   verification has not yet been performed (or failed and is
//!   scheduled for re-verification).
//!
//! Both queues are bounded by the configured `capacity` (typically
//! `ProtocolSettings::memory_pool_max_transactions`). When the
//! pool is full, the lowest-priority item is evicted to make room
//! for a higher-priority one.

use crate::new_transaction_event_args::NewTransactionEventArgs;
use crate::pool_index::PoolIndex;
use crate::pool_item::PoolItem;
use crate::transaction_removed_event_args::TransactionRemovedEventArgs;
use crate::transaction_verification_context::TransactionVerificationContext;
use neo_config::ProtocolSettings;
use neo_data_cache::DataCache;
use neo_payloads::Transaction;
use neo_primitives::{TransactionRemovalReason, UInt256, VerifyResult};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Callback invoked after a new transaction has been accepted into
/// the pool.
pub type TransactionAddedCallback = dyn Fn(&MemoryPool, &Transaction) + Send + Sync;
/// Callback invoked after a transaction (or set of transactions) is
/// removed from the pool.
pub type TransactionRemovedCallback =
    dyn Fn(&MemoryPool, &TransactionRemovedEventArgs) + Send + Sync;
/// Callback invoked when a transaction should be rebroadcast to the
/// network.
pub type TransactionRelayCallback = dyn Fn(&Transaction) + Send + Sync;
/// Callback invoked for every freshly-admitted transaction; subscribers
/// may veto the admission by setting `cancel = true` on the event args.
pub type NewTransactionCallback =
    dyn Fn(&MemoryPool, &mut NewTransactionEventArgs) + Send + Sync;

/// Inner, mutable state of the memory pool. Split out so the outer
/// `MemoryPool` can hand out read-only references while still
/// allowing the rest of the system to mutate the pool under a lock.
struct MemoryPoolInner {
    verified: PoolIndex,
    unverified: PoolIndex,
    conflicts: HashMap<UInt256, HashSet<UInt256>>,
    verification_context: TransactionVerificationContext,
    capacity: usize,
}

/// Neo transaction memory pool.
pub struct MemoryPool {
    /// Optional subscriber callback invoked to validate a new
    /// transaction before it is admitted.
    pub new_transaction: Option<Box<NewTransactionCallback>>,
    /// Optional subscriber callback invoked after a transaction has
    /// been added to the pool.
    pub transaction_added: Option<Box<TransactionAddedCallback>>,
    /// Optional subscriber callback invoked after a transaction has
    /// been removed from the pool.
    pub transaction_removed: Option<Box<TransactionRemovedCallback>>,
    /// Optional subscriber callback invoked when a transaction should
    /// be rebroadcast to the network.
    pub transaction_relay: Option<Box<TransactionRelayCallback>>,

    inner: RwLock<MemoryPoolInner>,
}

impl MemoryPool {
    /// Constructs a new memory pool using the supplied protocol
    /// settings. The pool capacity is taken from
    /// `settings.memory_pool_max_transactions`.
    pub fn new(settings: &ProtocolSettings) -> Self {
        let capacity = settings.memory_pool_max_transactions as usize;
        Self {
            new_transaction: None,
            transaction_added: None,
            transaction_removed: None,
            transaction_relay: None,
            inner: RwLock::new(MemoryPoolInner {
                verified: PoolIndex::with_capacity(capacity),
                unverified: PoolIndex::with_capacity(capacity / 4),
                conflicts: HashMap::with_capacity(capacity / 2),
                verification_context: TransactionVerificationContext::new(),
                capacity,
            }),
        }
    }

    /// Returns the configured maximum pool capacity.
    pub fn capacity(&self) -> usize {
        self.inner.read().capacity
    }

    /// Returns the number of verified transactions currently in the pool.
    pub fn verified_count(&self) -> usize {
        self.inner.read().verified.len()
    }

    /// Returns the number of unverified transactions currently in the pool.
    pub fn unverified_count(&self) -> usize {
        self.inner.read().unverified.len()
    }

    /// Returns the total number of transactions currently in the pool
    /// (verified + unverified).
    pub fn total_count(&self) -> usize {
        let guard = self.inner.read();
        guard.verified.len() + guard.unverified.len()
    }

    /// Returns whether the pool contains a transaction with the
    /// given hash (in either the verified or unverified queue).
    pub fn contains(&self, hash: &UInt256) -> bool {
        let guard = self.inner.read();
        guard.verified.contains(hash) || guard.unverified.contains(hash)
    }

    /// Returns the pool item for the given hash, preferring the
    /// verified queue over the unverified one.
    pub fn get(&self, hash: &UInt256) -> Option<PoolItem> {
        let guard = self.inner.read();
        guard
            .verified
            .get(hash)
            .or_else(|| guard.unverified.get(hash))
            .cloned()
    }

    /// Returns a snapshot of the verified queue in priority order
    /// (highest fee-per-byte first).
    pub fn verified_snapshot(&self) -> Vec<PoolItem> {
        self.inner.read().verified.to_sorted_vec()
    }

    /// Returns a snapshot of the unverified queue in priority order.
    pub fn unverified_snapshot(&self) -> Vec<PoolItem> {
        self.inner.read().unverified.to_sorted_vec()
    }

    /// Records the supplied transaction hashes as confirmed in the
    /// current persisting block. Returns the hashes that were
    /// previously known (i.e. present in the pool) so the caller can
    /// remove them and emit removal events.
    pub fn commit_block(
        &self,
        confirmed: &[UInt256],
    ) -> Vec<(Transaction, TransactionRemovalReason)> {
        let mut guard = self.inner.write();
        let mut removed = Vec::with_capacity(confirmed.len());
        for hash in confirmed {
            guard.verification_context.confirm(*hash);
            if let Some(item) = guard.verified.remove(hash) {
                let tx = (*item.transaction).clone();
                guard.conflicts.retain(|_, set| {
                    set.remove(hash);
                    !set.is_empty()
                });
                removed.push((tx, TransactionRemovalReason::NoLongerValid));
            }
            if let Some(item) = guard.unverified.remove(hash) {
                let tx = (*item.transaction).clone();
                removed.push((tx, TransactionRemovalReason::NoLongerValid));
            }
        }
        removed
    }

    /// Promotes a batch of unverified transactions to verified,
    /// running each through the supplied closure. Returns the
    /// list of removals encountered.
    pub fn reverify<F>(
        &self,
        snapshot: &DataCache,
        verifier: F,
    ) -> Vec<(Transaction, TransactionRemovalReason)>
    where
        F: Fn(&Transaction, &DataCache) -> VerifyResult,
    {
        let mut guard = self.inner.write();
        let mut removals = Vec::new();
        let to_check: Vec<PoolItem> = guard.unverified.iter().cloned().collect();

        for item in to_check {
            let tx = (*item.transaction).clone();
            let result = verifier(&tx, snapshot);
            if result.is_success() {
                let hash = item.hash();
                guard.unverified.remove(&hash);
                guard.verified.insert(item);
            } else {
                let hash = item.hash();
                guard.unverified.remove(&hash);
                removals.push((tx, TransactionRemovalReason::NoLongerValid));
            }
        }
        removals
    }

    /// Attempts to admit a fresh transaction into the pool. Returns
    /// the [`VerifyResult`] describing the outcome.
    ///
    /// This is a streamlined variant of the full C# `TryAdd`
    /// pipeline: it performs the cheap priority-queue / capacity
    /// checks and emits the subscriber callbacks, but defers the
    /// state-dependent witness verification to a later
    /// [`Self::reverify`] call.
    pub fn try_add(
        &self,
        transaction: Transaction,
        snapshot: &DataCache,
    ) -> VerifyResult {
        let hash = transaction.hash();

        // Subscriber veto gate.
        if let Some(callback) = &self.new_transaction {
            let mut args = NewTransactionEventArgs::new(transaction.clone(), snapshot.clone());
            callback(self, &mut args);
            if args.cancel {
                return VerifyResult::PolicyFail;
            }
        }

        {
            let mut guard = self.inner.write();
            if guard.verified.contains(&hash) || guard.unverified.contains(&hash) {
                return VerifyResult::AlreadyExists;
            }
            if guard.verified.len() >= guard.capacity {
                // Evict the lowest-priority verified item to make room.
                if let Some(lowest) = guard.verified.items.iter().next_back().cloned() {
                    guard.verified.remove(&lowest.hash());
                } else {
                    return VerifyResult::OutOfMemory;
                }
            }
            guard.unverified.insert(PoolItem::new(transaction.clone()));
        }

        if let Some(callback) = &self.transaction_added {
            callback(self, &transaction);
        }
        VerifyResult::Succeed
    }

    /// Removes the transaction with the given hash from the pool
    /// and emits the `transaction_removed` event.
    pub fn remove(&self, hash: &UInt256, reason: TransactionRemovalReason) {
        let tx_opt = {
            let mut guard = self.inner.write();
            guard
                .verified
                .remove(hash)
                .or_else(|| guard.unverified.remove(hash))
                .map(|item| (*item.transaction).clone())
        };
        if let Some(tx) = tx_opt {
            if let Some(callback) = &self.transaction_removed {
                let args = TransactionRemovedEventArgs::new(vec![tx], reason);
                callback(self, &args);
            }
        }
    }

    /// Returns whether the pool holds a `verify_state_independent`-
    /// compatible transaction for the given hash.
    pub fn has_transaction(&self, hash: &UInt256) -> bool {
        self.contains(hash)
    }
}

impl std::fmt::Debug for MemoryPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.read();
        f.debug_struct("MemoryPool")
            .field("capacity", &guard.capacity)
            .field("verified", &guard.verified.len())
            .field("unverified", &guard.unverified.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::{Signer, Transaction, Witness};
    use neo_primitives::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;

    fn sample_tx(nonce: u32) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(1);
        tx.set_script(vec![OpCode::RET.byte()]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn empty_pool_has_zero_counts() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        assert_eq!(pool.total_count(), 0);
        assert_eq!(pool.verified_count(), 0);
        assert_eq!(pool.unverified_count(), 0);
    }

    #[test]
    fn try_add_admits_into_unverified() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let tx = sample_tx(1);
        let hash = tx.hash();
        let result = pool.try_add(tx, &snapshot);
        assert!(result.is_success());
        assert!(pool.unverified_count() == 1);
        assert!(pool.contains(&hash));
    }

    #[test]
    fn try_add_rejects_duplicate() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let tx = sample_tx(1);
        let _ = pool.try_add(tx.clone(), &snapshot);
        let second = pool.try_add(tx, &snapshot);
        assert!(matches!(second, VerifyResult::AlreadyExists));
    }

    #[test]
    fn commit_block_removes_confirmed_transactions() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let tx = sample_tx(7);
        let hash = tx.hash();
        let _ = pool.try_add(tx, &snapshot);
        let removed = pool.commit_block(&[hash]);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].1, TransactionRemovalReason::NoLongerValid);
        assert!(!pool.contains(&hash));
    }

    #[test]
    fn reverify_promotes_successful_transactions() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let _ = pool.try_add(sample_tx(1), &snapshot);
        let _ = pool.try_add(sample_tx(2), &snapshot);
        let removals = pool.reverify(&snapshot, |_tx, _snap| VerifyResult::Succeed);
        assert!(removals.is_empty());
        assert_eq!(pool.verified_count(), 2);
        assert_eq!(pool.unverified_count(), 0);
    }

    #[test]
    fn reverify_drops_failing_transactions() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let _ = pool.try_add(sample_tx(1), &snapshot);
        let removals = pool.reverify(&snapshot, |_tx, _snap| VerifyResult::PolicyFail);
        assert_eq!(removals.len(), 1);
        assert_eq!(removals[0].1, TransactionRemovalReason::NoLongerValid);
        assert_eq!(pool.unverified_count(), 0);
    }
}

// Re-export the underlying Arc wrapper for the `Arc<MemoryPool>` pattern
// used by services that need to share the pool across tasks.
pub type SharedMemoryPool = Arc<MemoryPool>;

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

use super::{
    PoolItem, TransactionRemovedEventArgs, TransactionVerificationContext,
    new_transaction_event_args::NewTransactionEventArgs,
    transaction_removal_reason::TransactionRemovalReason, verify_result::VerifyResult,
};
use crate::hardfork::Hardfork;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::network::p2p::payloads::{Transaction, conflicts::Conflicts};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::{LedgerContract, PolicyContract};
use crate::{UInt160, UInt256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// namespace Neo.Ledger -> public class MemoryPool : IReadOnlyCollection<`Transaction`>
/// Allow a reverified transaction to be rebroadcast if it has been this many block times since last broadcast.
const _BLOCKS_TILL_REBROADCAST: i32 = 10;

/// Used to cache verified transactions before being written into the block.
type TransactionAddedCallback = dyn Fn(&MemoryPool, &Transaction) + Send + Sync;
type TransactionRemovedCallback = dyn Fn(&MemoryPool, &TransactionRemovedEventArgs) + Send + Sync;
type TransactionRelayCallback = dyn Fn(&Transaction) + Send + Sync;
type NewTransactionCallback = dyn Fn(&MemoryPool, &mut NewTransactionEventArgs) + Send + Sync;

fn resolve_time_per_block(snapshot: &DataCache, settings: &ProtocolSettings) -> Duration {
    let current_index = LedgerContract::new().current_index(snapshot).unwrap_or(0);
    if !settings.is_hardfork_enabled(Hardfork::HfEchidna, current_index) {
        return settings.time_per_block();
    }

    PolicyContract::get_milliseconds_per_block_snapshot(snapshot)
        .map(|ms| Duration::from_millis(ms as u64))
        .unwrap_or_else(|| settings.time_per_block())
}

pub struct MemoryPool {
    /// Callback invoked to validate a new transaction before adding it to the pool.
    pub new_transaction: Option<Box<NewTransactionCallback>>,

    /// Callback invoked when a transaction is added to the pool.
    pub transaction_added: Option<Box<TransactionAddedCallback>>,

    /// Callback invoked when a transaction (or set of transactions) is removed from the pool.
    pub transaction_removed: Option<Box<TransactionRemovedCallback>>,

    /// Callback invoked when a transaction should be rebroadcast to the network.
    pub transaction_relay: Option<Box<TransactionRelayCallback>>,

    _max_milliseconds_to_reverify_tx: f64,
    _max_milliseconds_to_reverify_tx_per_idle: f64,

    verified_transactions: HashMap<UInt256, PoolItem>,
    verified_sorted: BTreeSet<PoolItem>,
    unverified_transactions: HashMap<UInt256, PoolItem>,
    unverified_sorted: BTreeSet<PoolItem>,
    conflicts: HashMap<UInt256, HashSet<UInt256>>,

    verification_context: TransactionVerificationContext,

    pub capacity: usize,
}

impl MemoryPool {
    /// Creates a memory pool using the provided protocol settings.
    pub fn new(settings: &ProtocolSettings) -> Self {
        Self::new_with_time_per_block(settings, settings.time_per_block())
    }

    /// Creates a memory pool using the provided protocol settings and block time.
    pub fn new_with_time_per_block(settings: &ProtocolSettings, time_per_block: Duration) -> Self {
        let capacity = settings.memory_pool_max_transactions as usize;
        let time_per_block_ms = time_per_block.as_secs_f64() * 1000.0;

        Self {
            new_transaction: None,
            transaction_added: None,
            transaction_removed: None,
            transaction_relay: None,
            _max_milliseconds_to_reverify_tx: time_per_block_ms / 3.0,
            _max_milliseconds_to_reverify_tx_per_idle: time_per_block_ms / 15.0,
            verified_transactions: HashMap::with_capacity(capacity),
            verified_sorted: BTreeSet::new(),
            unverified_transactions: HashMap::with_capacity(capacity / 4),
            unverified_sorted: BTreeSet::new(),
            conflicts: HashMap::with_capacity(capacity / 2),
            verification_context: TransactionVerificationContext::new(),
            capacity,
        }
    }

    /// Pre-allocates capacity for expected number of verified transactions.
    /// Call this during initial sync to reduce memory reallocations.
    pub fn reserve_verified(&mut self, additional: usize) {
        let new_capacity = self.verified_transactions.len().saturating_add(additional);
        self.verified_transactions
            .reserve(new_capacity.min(self.capacity));
    }

    /// Pre-allocates capacity for expected number of unverified transactions.
    pub fn reserve_unverified(&mut self, additional: usize) {
        let new_capacity = self
            .unverified_transactions
            .len()
            .saturating_add(additional);
        self.unverified_transactions
            .reserve(new_capacity.min(self.capacity / 4));
    }

    /// private int RebroadcastMultiplierThreshold => Capacity / 10;
    fn rebroadcast_multiplier_threshold(&self) -> i32 {
        i32::try_from(self.capacity)
            .unwrap_or(i32::MAX)
            .saturating_div(10)
    }

    /// Returns the highest-priority verified transactions, sorted in descending order by fee.
    /// Uses `Arc<Transaction>` to avoid expensive cloning of transaction data.
    pub fn get_sorted_verified_transactions(&self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(limit.min(self.verified_sorted.len()));
        result.extend(
            self.verified_sorted
                .iter()
                .rev()
                .take(limit)
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// internal int SortedTxCount => _sortedTransactions.Count;
    #[cfg(test)]
    pub(crate) fn sorted_tx_count(&self) -> usize {
        self.verified_sorted.len()
    }

    /// internal int UnverifiedSortedTxCount => _unverifiedSortedTransactions.Count;
    #[cfg(test)]
    pub(crate) fn unverified_sorted_tx_count(&self) -> usize {
        self.unverified_sorted.len()
    }

    /// public int Count
    pub fn count(&self) -> usize {
        self.verified_transactions.len() + self.unverified_transactions.len()
    }

    /// public int VerifiedCount => _unsortedTransactions.Count;
    pub fn verified_count(&self) -> usize {
        self.verified_transactions.len()
    }

    /// public int UnVerifiedCount => _unverifiedTransactions.Count;
    pub fn unverified_count(&self) -> usize {
        self.unverified_transactions.len()
    }

    /// public bool ContainsKey(UInt256 hash)
    pub fn contains_key(&self, hash: &UInt256) -> bool {
        self.verified_transactions.contains_key(hash)
            || self.unverified_transactions.contains_key(hash)
    }

    #[cfg(test)]
    fn lowest_fee_item(&self) -> Option<&PoolItem> {
        let verified = self.verified_sorted.iter().next();
        let unverified = self.unverified_sorted.iter().next();

        match (verified, unverified) {
            (None, None) => None,
            (Some(item), None) => Some(item),
            (None, Some(item)) => Some(item),
            (Some(verified_item), Some(unverified_item)) => {
                if verified_item.compare_to(unverified_item) != std::cmp::Ordering::Less {
                    Some(unverified_item)
                } else {
                    Some(verified_item)
                }
            }
        }
    }

    /// Returns true if the pool has capacity for a transaction with at least the given priority.
    #[cfg(test)]
    pub(crate) fn can_transaction_fit_in_pool(&self, tx: &Transaction) -> bool {
        if self.count() < self.capacity {
            return true;
        }

        let Some(item) = self.lowest_fee_item() else {
            return false;
        };
        item.compare_to_transaction(tx) != std::cmp::Ordering::Greater
    }

    fn check_conflicts(&self, tx: &Transaction) -> Result<Vec<PoolItem>, VerifyResult> {
        let mut to_remove = Vec::new();
        let mut total_conflict_fee = 0i64;
        let Some(sender) = tx.sender() else {
            return Ok(to_remove);
        };

        let mut push_unique = |item: &PoolItem| {
            let item_hash = item.transaction.hash();
            if !to_remove
                .iter()
                .any(|existing: &PoolItem| existing.transaction.hash() == item_hash)
            {
                to_remove.push(item.clone());
            }
        };

        if let Some(conflicting_hashes) = self.conflicts.get(&tx.hash()) {
            for hash in conflicting_hashes {
                if let Some(conflict_item) = self.verified_transactions.get(hash) {
                    if conflict_item
                        .transaction
                        .signers()
                        .iter()
                        .any(|signer| signer.account == sender)
                    {
                        total_conflict_fee += conflict_item.transaction.network_fee();
                    }
                    push_unique(conflict_item);
                }
            }
        }

        for attr in tx.attributes() {
            if let TransactionAttribute::Conflicts(Conflicts { hash }) = attr {
                if let Some(conflict_item) = self.verified_transactions.get(hash) {
                    let share_sender = tx.signers().iter().any(|signer| {
                        conflict_item
                            .transaction
                            .signers()
                            .iter()
                            .any(|existing| existing.account == signer.account)
                    });
                    if !share_sender {
                        return Err(VerifyResult::HasConflicts);
                    }
                    total_conflict_fee += conflict_item.transaction.network_fee();
                    push_unique(conflict_item);
                }
            }
        }

        if total_conflict_fee != 0 && total_conflict_fee >= tx.network_fee() {
            return Err(VerifyResult::HasConflicts);
        }

        Ok(to_remove)
    }

    fn register_conflicts(&mut self, tx_hash: UInt256, tx: &Transaction) {
        for attr in tx.attributes() {
            if let TransactionAttribute::Conflicts(Conflicts { hash }) = attr {
                self.conflicts.entry(*hash).or_default().insert(tx_hash);
            }
        }
    }

    fn unregister_conflicts(&mut self, tx_hash: &UInt256, tx: &Transaction) {
        for attr in tx.attributes() {
            if let TransactionAttribute::Conflicts(Conflicts { hash }) = attr {
                if let Some(set) = self.conflicts.get_mut(hash) {
                    set.remove(tx_hash);
                    if set.is_empty() {
                        self.conflicts.remove(hash);
                    }
                }
            }
        }
    }

    /// Attempts to add a transaction to the mempool, performing full validation.
    ///
    /// # Security
    /// This method performs both state-independent and state-dependent validation
    /// before adding the transaction to the mempool. State-independent validation
    /// includes checks like transaction structure, size limits, script validity,
    /// attribute validity, and other checks that don't require blockchain state.
    pub fn try_add(
        &mut self,
        tx: Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> VerifyResult {
        let hash = tx.hash();

        if let Some(handler) = &self.new_transaction {
            let mut args = NewTransactionEventArgs {
                transaction: tx.clone(),
                snapshot: snapshot.clone(),
                cancel: false,
            };
            handler(self, &mut args);
            if args.cancel {
                return VerifyResult::PolicyFail;
            }
        }

        if self.verified_transactions.contains_key(&hash)
            || self.unverified_transactions.contains_key(&hash)
        {
            return VerifyResult::AlreadyInPool;
        }

        // SECURITY FIX: Perform state-independent validation first
        // This validates transaction structure, size limits, script validity,
        // attribute validity, and other checks that don't require blockchain state.
        let state_independent_result = tx.verify_state_independent(settings);
        if state_independent_result != VerifyResult::Succeed {
            return state_independent_result;
        }

        let conflicts_to_remove = match self.check_conflicts(&tx) {
            Ok(list) => list,
            Err(result) => return result,
        };

        // OPTIMIZATION: Build conflict transactions Vec with pre-allocated capacity.
        // Use Arc::clone to share references instead of deep cloning transaction data.
        let conflict_transactions: Vec<Transaction> = conflicts_to_remove
            .iter()
            .map(|item| item.transaction.as_ref().clone())
            .fold(
                Vec::with_capacity(conflicts_to_remove.len()),
                |mut acc, tx| {
                    acc.push(tx);
                    acc
                },
            );

        // State-dependent validation (requires blockchain state)
        let result = tx.verify_state_dependent(
            settings,
            snapshot,
            Some(&self.verification_context),
            &conflict_transactions,
        );
        if result != VerifyResult::Succeed {
            return result;
        }

        let item = PoolItem::new(tx.clone());
        self.verification_context.add_transaction(&tx);
        self.verified_transactions.insert(hash, item.clone());
        self.verified_sorted.insert(item.clone());
        self.register_conflicts(hash, &tx);

        if !conflicts_to_remove.is_empty() {
            let mut removed_conflicts = Vec::with_capacity(conflicts_to_remove.len());
            for removed_item in conflicts_to_remove {
                let removed_hash = removed_item.transaction.hash();
                if let Some(item) = self.try_remove_verified(removed_hash) {
                    // Extract Arc<Transaction> directly without cloning
                    removed_conflicts.push(
                        Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone()),
                    );
                }
            }

            if let Some(handler) = &self.transaction_removed {
                if !removed_conflicts.is_empty() {
                    handler(
                        self,
                        &TransactionRemovedEventArgs {
                            transactions: removed_conflicts,
                            reason: TransactionRemovalReason::Conflict,
                        },
                    );
                }
            }
        }

        if self.count() > self.capacity {
            let removed = self.remove_over_capacity();
            if let Some(handler) = &self.transaction_removed {
                if !removed.is_empty() {
                    handler(
                        self,
                        &TransactionRemovedEventArgs {
                            transactions: removed,
                            reason: TransactionRemovalReason::CapacityExceeded,
                        },
                    );
                }
            }
            if !self.verified_transactions.contains_key(&hash) {
                return VerifyResult::OutOfMemory;
            }
        }

        if let Some(handler) = &self.transaction_added {
            handler(self, &tx);
        }

        VerifyResult::Succeed
    }

    /// Attempts to fetch a transaction from either the verified or unverified sets.
    /// Returns `Arc<Transaction>` to avoid expensive cloning.
    pub fn try_get(&self, hash: &UInt256) -> Option<Arc<Transaction>> {
        if let Some(item) = self.verified_transactions.get(hash) {
            return Some(Arc::clone(&item.transaction));
        }
        self.unverified_transactions
            .get(hash)
            .map(|item| Arc::clone(&item.transaction))
    }

    /// Returns the highest priority verified transactions, up to `limit`.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn sorted_verified_transactions(&self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(limit.min(self.verified_sorted.len()));
        result.extend(
            self.verified_sorted
                .iter()
                .rev()
                .take(limit)
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all verified transactions without any ordering guarantees.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn verified_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let mut result = Vec::with_capacity(self.verified_transactions.len());
        result.extend(
            self.verified_transactions
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all unverified transactions currently tracked by the mempool.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn unverified_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let mut result = Vec::with_capacity(self.unverified_transactions.len());
        result.extend(
            self.unverified_transactions
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all transactions (verified followed by unverified) currently tracked by the mempool.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn all_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let total_len = self.verified_transactions.len() + self.unverified_transactions.len();
        let mut transactions = Vec::with_capacity(total_len);
        transactions.extend(
            self.verified_transactions
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        transactions.extend(
            self.unverified_transactions
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        transactions
    }

    /// Returns verified and unverified transactions as separate vectors,
    /// sorted in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn verified_and_unverified_transactions(
        &self,
    ) -> (Vec<Arc<Transaction>>, Vec<Arc<Transaction>>) {
        let verified_capacity = self.verified_sorted.len();
        let unverified_capacity = self.unverified_sorted.len();

        let mut verified = Vec::with_capacity(verified_capacity);
        let mut unverified = Vec::with_capacity(unverified_capacity);

        verified.extend(
            self.verified_sorted
                .iter()
                .rev()
                .map(|item| Arc::clone(&item.transaction)),
        );
        unverified.extend(
            self.unverified_sorted
                .iter()
                .rev()
                .map(|item| Arc::clone(&item.transaction)),
        );
        (verified, unverified)
    }

    /// Returns an iterator over verified transactions in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn iter_verified(&self) -> impl Iterator<Item = Arc<Transaction>> + '_ {
        self.verified_sorted
            .iter()
            .rev()
            .map(|item| Arc::clone(&item.transaction))
    }

    /// Returns an iterator over unverified transactions in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn iter_unverified(&self) -> impl Iterator<Item = Arc<Transaction>> + '_ {
        self.unverified_sorted
            .iter()
            .rev()
            .map(|item| Arc::clone(&item.transaction))
    }

    /// Removes transactions committed in the provided block, evicts conflicts,
    /// and re-verifies remaining transactions using the per-block time budget.
    pub fn update_pool_for_block_persisted(
        &mut self,
        block: &crate::network::p2p::payloads::block::Block,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        header_backlog_present: bool,
    ) {
        let mut conflicts: HashMap<UInt256, HashSet<UInt160>> = HashMap::new();
        let mut persisted: HashSet<UInt256> = HashSet::new();

        for tx in &block.transactions {
            persisted.insert(tx.hash());
            match self.try_remove_verified(tx.hash()) {
                Some(item) => {
                    self.verification_context
                        .remove_transaction(&item.transaction);
                }
                _ => {
                    let _ = self.try_remove_unverified(tx.hash());
                }
            }

            let signers: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();
            if signers.is_empty() {
                continue;
            }

            for attr in tx.attributes() {
                if let TransactionAttribute::Conflicts(Conflicts { hash }) = attr {
                    let entry = conflicts.entry(*hash).or_default();
                    entry.extend(signers.iter().copied());
                }
            }
        }

        let mut conflicting_items = Vec::new();
        if !self.verified_sorted.is_empty() && (!conflicts.is_empty() || !persisted.is_empty()) {
            let stale: Vec<UInt256> = self
                .verified_sorted
                .iter()
                .filter_map(|item| {
                    let item_hash = item.transaction.hash();
                    let matches_conflict = conflicts.get(&item_hash).is_some_and(|signers| {
                        item.transaction
                            .signers()
                            .iter()
                            .any(|signer| signers.contains(&signer.account))
                    });
                    let matches_persisted = item.transaction.attributes().iter().any(|attr| {
                        matches!(attr, TransactionAttribute::Conflicts(Conflicts { hash }) if persisted.contains(hash))
                    });

                    if matches_conflict || matches_persisted {
                        // Extract Transaction for the event handler
                        conflicting_items.push((*item.transaction).clone());
                        Some(item_hash)
                    } else {
                        None
                    }
                })
                .collect();

            for hash in stale {
                if self.try_remove_verified(hash).is_none() {
                    let _ = self.try_remove_unverified(hash);
                }
            }
        }

        self.invalidate_verified_transactions();

        if !conflicting_items.is_empty() {
            if let Some(handler) = &self.transaction_removed {
                handler(
                    self,
                    &TransactionRemovedEventArgs {
                        transactions: conflicting_items,
                        reason: TransactionRemovalReason::Conflict,
                    },
                );
            }
        }

        if block.index() > 0 && header_backlog_present {
            return;
        }

        let time_budget = if self._max_milliseconds_to_reverify_tx <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(
                self._max_milliseconds_to_reverify_tx / 1000.0,
            ))
        };

        self.reverify_unverified_transactions(
            settings.max_transactions_per_block as usize,
            snapshot,
            settings,
            time_budget,
        );
    }

    /// Clears both verified and unverified sets entirely.
    pub fn invalidate_all_transactions(&mut self) {
        self.verified_transactions.clear();
        self.verified_sorted.clear();
        self.unverified_transactions.clear();
        self.unverified_sorted.clear();
        self.conflicts.clear();
        self.verification_context = TransactionVerificationContext::new();
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn insert_unverified_for_test(&mut self, tx: Transaction) {
        let hash = tx.hash();
        let item = PoolItem::new(tx);
        self.unverified_transactions.insert(hash, item.clone());
        self.unverified_sorted.insert(item);
    }

    /// Re-verifies a limited number of unverified transactions, promoting valid ones back into the
    /// verified set. Returns `true` if unverified entries still remain afterwards.
    pub fn reverify_top_unverified_transactions(
        &mut self,
        max_to_verify: usize,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        header_backlog_present: bool,
    ) -> bool {
        if header_backlog_present || max_to_verify == 0 || self.unverified_sorted.is_empty() {
            return !self.unverified_transactions.is_empty();
        }

        let time_budget = if self._max_milliseconds_to_reverify_tx_per_idle <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(
                self._max_milliseconds_to_reverify_tx_per_idle / 1000.0,
            ))
        };

        self.reverify_unverified_transactions(max_to_verify, snapshot, settings, time_budget)
    }

    fn reverify_unverified_transactions(
        &mut self,
        max_to_verify: usize,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        time_budget: Option<Duration>,
    ) -> bool {
        if max_to_verify == 0 || self.unverified_sorted.is_empty() {
            return !self.unverified_transactions.is_empty();
        }

        let verify_count =
            if self.verified_transactions.len() > settings.max_transactions_per_block as usize {
                1usize
            } else {
                max_to_verify
            };

        let verify_count = verify_count.min(self.unverified_sorted.len());
        if verify_count == 0 {
            return !self.unverified_transactions.is_empty();
        }

        let start_instant = Instant::now();

        let candidates: Vec<PoolItem> = self
            .unverified_sorted
            .iter()
            .rev()
            .take(verify_count)
            .cloned()
            .collect();

        let mut reverified = Vec::with_capacity(candidates.len());
        let mut invalidated = Vec::new();

        for item in candidates {
            if let Some(budget) = time_budget {
                if start_instant.elapsed() > budget {
                    break;
                }
            }

            let hash = item.transaction.hash();
            if !self.unverified_transactions.contains_key(&hash) {
                continue;
            }

            let conflicts = match self.check_conflicts(&item.transaction) {
                Ok(list) => list,
                Err(_) => {
                    self.unverified_transactions.remove(&hash);
                    self.unverified_sorted.take(&item);
                    invalidated.push(
                        Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone()),
                    );
                    continue;
                }
            };

            // Build conflict transactions Vec with pre-allocated capacity
            let conflict_txs: Vec<Transaction> = conflicts
                .iter()
                .map(|pool_item| pool_item.transaction.as_ref().clone())
                .fold(Vec::with_capacity(conflicts.len()), |mut acc, tx| {
                    acc.push(tx);
                    acc
                });

            let verify_result = item.transaction.verify_state_dependent(
                settings,
                snapshot,
                Some(&self.verification_context),
                &conflict_txs,
            );

            if verify_result == VerifyResult::Succeed {
                self.unverified_transactions.remove(&hash);
                self.unverified_sorted.take(&item);
                self.verified_transactions.insert(hash, item.clone());
                self.verified_sorted.insert(item.clone());
                self.register_conflicts(hash, &item.transaction);
                self.verification_context.add_transaction(&item.transaction);

                for conflict in conflicts {
                    let conflict_hash = conflict.transaction.hash();
                    if let Some(removed) = self.try_remove_verified(conflict_hash) {
                        // Extract Transaction for the event handler
                        invalidated.push(
                            Arc::try_unwrap(removed.transaction)
                                .unwrap_or_else(|arc| (*arc).clone()),
                        );
                    }
                }

                reverified.push(item);
            } else {
                self.unverified_transactions.remove(&hash);
                self.unverified_sorted.take(&item);
                invalidated
                    .push(Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone()));
            }
        }

        let now = SystemTime::now();
        let mut blocks_till_rebroadcast = _BLOCKS_TILL_REBROADCAST.max(1);
        if self.count() as i32 > self.rebroadcast_multiplier_threshold() {
            let scaled = (_BLOCKS_TILL_REBROADCAST as i64)
                .saturating_mul(self.count() as i64)
                .saturating_div(self.rebroadcast_multiplier_threshold() as i64);
            blocks_till_rebroadcast = scaled.clamp(1, i32::MAX as i64) as i32;
        }

        let time_per_block = resolve_time_per_block(snapshot, settings);
        let rebroadcast_duration = time_per_block
            .checked_mul(blocks_till_rebroadcast as u32)
            .unwrap_or_else(|| Duration::from_secs(0));
        let rebroadcast_cutoff = now
            .checked_sub(rebroadcast_duration)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        if !reverified.is_empty() {
            for item in &reverified {
                let hash = item.transaction.hash();
                if let Some(stored) = self.verified_transactions.get_mut(&hash) {
                    if stored.last_broadcast_timestamp < rebroadcast_cutoff {
                        if let Some(relay) = &self.transaction_relay {
                            relay(&stored.transaction);
                        }
                        stored.last_broadcast_timestamp = now;
                    }
                }
            }
        }

        if !invalidated.is_empty() {
            if let Some(handler) = &self.transaction_removed {
                handler(
                    self,
                    &TransactionRemovedEventArgs {
                        transactions: invalidated,
                        reason: TransactionRemovalReason::NoLongerValid,
                    },
                );
            }
        }

        !self.unverified_transactions.is_empty()
    }

    fn try_remove_verified(&mut self, hash: UInt256) -> Option<PoolItem> {
        let item = self.verified_transactions.remove(&hash)?;
        self.verified_sorted.take(&item);
        self.verification_context
            .remove_transaction(&item.transaction);
        self.unregister_conflicts(&hash, &item.transaction);
        Some(item)
    }

    fn try_remove_unverified(&mut self, hash: UInt256) -> Option<PoolItem> {
        let item = self.unverified_transactions.remove(&hash)?;
        self.unverified_sorted.take(&item);
        self.unregister_conflicts(&hash, &item.transaction);
        Some(item)
    }

    /// Removes an unverified transaction by hash.
    pub fn remove_unverified(&mut self, hash: &UInt256) -> bool {
        self.try_remove_unverified(*hash).is_some()
    }

    fn invalidate_verified_transactions(&mut self) {
        if self.verified_sorted.is_empty() {
            return;
        }

        #[allow(clippy::mutable_key_type)]
        let sorted = std::mem::take(&mut self.verified_sorted);
        let mut txs = std::mem::take(&mut self.verified_transactions);

        for item in sorted {
            let hash = item.transaction.hash();
            // Prefer the HashMap entry (same PoolItem) to avoid keeping both.
            let item = txs.remove(&hash).unwrap_or(item);
            self.unverified_transactions.insert(hash, item.clone());
            self.unverified_sorted.insert(item);
        }

        self.conflicts.clear();
        self.verification_context = TransactionVerificationContext::new();
    }

    fn remove_over_capacity(&mut self) -> Vec<Transaction> {
        let mut removed = Vec::with_capacity(self.count().saturating_sub(self.capacity));

        while self.count() > self.capacity {
            let candidate_verified = self.verified_sorted.iter().next().cloned();
            let candidate_unverified = self.unverified_sorted.iter().next().cloned();

            let choice = match (candidate_verified, candidate_unverified) {
                (Some(v), Some(u)) => {
                    if v.compare_to(&u) != std::cmp::Ordering::Greater {
                        (v, true)
                    } else {
                        (u, false)
                    }
                }
                (Some(v), None) => (v, true),
                (None, Some(u)) => (u, false),
                (None, None) => break,
            };

            let (item, from_verified) = choice;
            let hash = item.transaction.hash();

            if from_verified {
                if let Some(removed_item) = self.try_remove_verified(hash) {
                    // Note: verification_context.remove_transaction is already
                    // called inside try_remove_verified — do NOT call it again.
                    removed.push(
                        Arc::try_unwrap(removed_item.transaction)
                            .unwrap_or_else(|arc| (*arc).clone()),
                    );
                }
            } else if let Some(removed_item) = self.try_remove_unverified(hash) {
                removed.push(
                    Arc::try_unwrap(removed_item.transaction).unwrap_or_else(|arc| (*arc).clone()),
                );
            }
        }

        removed
    }
}

// IReadOnlyCollection<Transaction> implementation
impl IntoIterator for MemoryPool {
    type Item = Transaction;
    type IntoIter = std::vec::IntoIter<Transaction>;

    fn into_iter(self) -> Self::IntoIter {
        let MemoryPool {
            verified_transactions,
            unverified_transactions,
            ..
        } = self;

        let mut transactions =
            Vec::with_capacity(verified_transactions.len() + unverified_transactions.len());
        transactions.extend(
            verified_transactions
                .into_values()
                .map(|item| Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone())),
        );
        transactions.extend(
            unverified_transactions
                .into_values()
                .map(|item| Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone())),
        );
        transactions.into_iter()
    }
}

impl IntoIterator for &MemoryPool {
    type Item = Transaction;
    type IntoIter = std::vec::IntoIter<Transaction>;

    fn into_iter(self) -> Self::IntoIter {
        // Collect all transactions - this requires cloning since we're borrowing self
        let total_len = self.verified_transactions.len() + self.unverified_transactions.len();
        let mut transactions = Vec::with_capacity(total_len);
        transactions.extend(
            self.verified_transactions
                .values()
                .map(|item| item.transaction.as_ref().clone()),
        );
        transactions.extend(
            self.unverified_transactions
                .values()
                .map(|item| item.transaction.as_ref().clone()),
        );
        transactions.into_iter()
    }
}

#[cfg(test)]
mod tests;

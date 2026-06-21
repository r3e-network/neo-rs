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
use neo_payloads::Transaction;
use neo_primitives::{TransactionRemovalReason, UInt160, UInt256, VerifyResult};
use neo_storage::DataCache;
use num_bigint::BigInt;
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
pub type NewTransactionCallback = dyn Fn(&MemoryPool, &mut NewTransactionEventArgs) + Send + Sync;

/// Inner, mutable state of the memory pool. Split out so the outer
/// `MemoryPool` can hand out read-only references while still
/// allowing the rest of the system to mutate the pool under a lock.
struct MemoryPoolInner {
    verified: PoolIndex,
    unverified: PoolIndex,
    conflicts: HashMap<UInt256, HashSet<UInt256>>,
    verification_context: TransactionVerificationContext,
    /// C# `TransactionVerificationContext._senderFee`: the summed
    /// system+network fees of pooled transactions per sender, charged
    /// against the sender's GAS balance for every new admission.
    sender_fees: HashMap<UInt160, BigInt>,
    /// C# `TransactionVerificationContext._oracleResponses`: pooled
    /// `OracleResponse` ids, rejecting duplicate responses.
    oracle_responses: HashMap<u64, UInt256>,
    capacity: usize,
}

impl MemoryPoolInner {
    /// C# `TransactionVerificationContext.AddTransaction`.
    fn context_add(&mut self, tx: &Transaction) {
        if let Some(oracle) = oracle_response_id(tx) {
            self.oracle_responses.insert(oracle, tx.hash());
        }
        if let Some(sender) = tx.signers().first().map(|s| s.account) {
            let fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
            *self.sender_fees.entry(sender).or_default() += fee;
        }
    }

    /// C# `TransactionVerificationContext.RemoveTransaction`.
    fn context_remove(&mut self, tx: &Transaction) {
        if let Some(oracle) = oracle_response_id(tx) {
            self.oracle_responses.remove(&oracle);
        }
        if let Some(sender) = tx.signers().first().map(|s| s.account) {
            let fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
            if let Some(total) = self.sender_fees.get_mut(&sender) {
                *total -= fee;
                if *total <= BigInt::from(0) {
                    self.sender_fees.remove(&sender);
                }
            }
        }
    }

    /// C# `MemoryPool.CheckConflicts` (MemoryPool.cs:381): returns the pooled
    /// transactions that conflict with `tx` and must be evicted if `tx` is
    /// admitted, or `None` if `tx` does not fit — i.e. a transaction `tx`
    /// declares as a conflict shares no signer with it, or the conflicting
    /// pooled transactions out-fee `tx` (sum of their network fees ≥ `tx`'s).
    fn check_conflicts(&self, tx: &Transaction) -> Option<Vec<PoolItem>> {
        let tx_hash = tx.hash();
        let tx_sender = tx.signers().first().map(|s| s.account);
        let tx_accounts: HashSet<UInt160> = tx.signers().iter().map(|s| s.account).collect();
        let mut list: Vec<PoolItem> = Vec::new();
        let mut conflicts_fee_sum: i64 = 0;

        // Step 1: pooled txs that declared `tx.hash` in their Conflicts attrs.
        if let Some(conflicting) = self.conflicts.get(&tx_hash) {
            for hash in conflicting {
                if let Some(pooled) = self.verified.get(hash) {
                    if tx_sender.is_some_and(|s| {
                        pooled
                            .transaction
                            .signers()
                            .iter()
                            .any(|sig| sig.account == s)
                    }) {
                        conflicts_fee_sum =
                            conflicts_fee_sum.saturating_add(pooled.transaction.network_fee());
                    }
                    list.push(pooled.clone());
                }
            }
        }
        // Step 2: pooled txs that `tx` declares in its own Conflicts attrs.
        for hash in conflict_target_hashes(tx) {
            if let Some(pooled) = self.verified.get(&hash) {
                let pooled_accounts: HashSet<UInt160> = pooled
                    .transaction
                    .signers()
                    .iter()
                    .map(|s| s.account)
                    .collect();
                // Must share at least one signer to be a real conflict.
                if tx_accounts.is_disjoint(&pooled_accounts) {
                    return None;
                }
                conflicts_fee_sum =
                    conflicts_fee_sum.saturating_add(pooled.transaction.network_fee());
                list.push(pooled.clone());
            }
        }
        // `tx` must out-fee the sum of conflicting txs' network fees.
        if conflicts_fee_sum != 0 && conflicts_fee_sum >= tx.network_fee() {
            return None;
        }
        Some(list)
    }

    /// C# `MemoryPool.GetLowestFeeTransaction` + one step of `RemoveOverCapacity`:
    /// evict the single global lowest-fee pooled transaction, PREFERRING the
    /// unverified queue on a tie (C# returns the unverified min when
    /// `verifiedMin.CompareTo(unverifiedMin) >= 0`). Returns the evicted
    /// transaction, or `None` if both queues are empty.
    fn remove_lowest_fee(&mut self) -> Option<Transaction> {
        // `items` is a BTreeSet ascending by PoolItem::compare_to, so
        // `iter().next()` is the lowest-priority item of each queue.
        let unverified_min = self.unverified.items.iter().next().cloned();
        let verified_min = self.verified.items.iter().next().cloned();
        let from_unverified = match (&verified_min, &unverified_min) {
            (Some(v), Some(u)) => v.compare_to(u) != std::cmp::Ordering::Less,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => return None,
        };
        let item = if from_unverified {
            unverified_min?
        } else {
            verified_min?
        };
        let hash = item.hash();
        if from_unverified {
            self.unverified.remove(&hash);
        } else {
            self.verified.remove(&hash);
        }
        let dropped = (*item.transaction).clone();
        self.context_remove(&dropped);
        self.conflicts.retain(|_, set| {
            set.remove(&hash);
            !set.is_empty()
        });
        Some(dropped)
    }
}

/// The hashes a transaction declares as conflicting via its `Conflicts` attributes.
fn conflict_target_hashes(tx: &Transaction) -> Vec<UInt256> {
    tx.attributes()
        .iter()
        .filter_map(|attr| match attr {
            neo_payloads::TransactionAttribute::Conflicts(c) => Some(c.hash),
            _ => None,
        })
        .collect()
}

/// C# `TransactionVerificationContext.CheckTransaction` conflict rebate: the
/// summed system+network fees of the conflicts that will be evicted and whose
/// Sender (`Signers[0].Account`) equals `tx_sender`. Those fees no longer count
/// against the sender's pooled-fee allowance. Conflicts with a different first
/// signer (or none) are not rebated, mirroring C#'s
/// `conflictingTxs.Where(c => c.Sender.Equals(tx.Sender))`.
fn conflict_rebate(conflicts: &[PoolItem], tx_sender: Option<UInt160>) -> BigInt {
    conflicts
        .iter()
        .filter(|c| {
            tx_sender.is_some_and(|sender| {
                c.transaction.signers().first().map(|s| s.account) == Some(sender)
            })
        })
        .map(|c| {
            BigInt::from(c.transaction.system_fee()) + BigInt::from(c.transaction.network_fee())
        })
        .sum()
}

/// Returns the `OracleResponse` attribute id of `tx`, if any.
fn oracle_response_id(tx: &Transaction) -> Option<u64> {
    tx.attributes().iter().find_map(|attr| match attr {
        neo_payloads::TransactionAttribute::OracleResponse(resp) => Some(resp.id),
        _ => None,
    })
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

    /// Protocol settings used by transaction verification (network
    /// magic for signature checks, hardfork schedule, expiry window).
    settings: ProtocolSettings,
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
            settings: settings.clone(),
            inner: RwLock::new(MemoryPoolInner {
                verified: PoolIndex::with_capacity(capacity),
                unverified: PoolIndex::with_capacity(capacity / 4),
                conflicts: HashMap::with_capacity(capacity / 2),
                verification_context: TransactionVerificationContext::new(),
                sender_fees: HashMap::new(),
                oracle_responses: HashMap::new(),
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

    /// Returns the verified pool item for the given hash.
    pub fn get_verified(&self, hash: &UInt256) -> Option<PoolItem> {
        self.inner.read().verified.get(hash).cloned()
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

    /// Updates the pool after a block is persisted, mirroring C#
    /// `MemoryPool.UpdatePoolForBlockPersisted`:
    ///
    /// 1. removes every transaction that was mined in the block from the
    ///    verified/unverified queues (so it is no longer served to peers or
    ///    re-proposed by the consensus driver) and decrements its sender-fee
    ///    accounting in the verification context;
    /// 2. evicts verified pooled transactions that conflict with the persisted
    ///    ones, in both directions — a pooled tx whose hash is named by a
    ///    persisted tx's `Conflicts` attribute (with an intersecting signer), or
    ///    whose own `Conflicts` attribute names a persisted tx — fired as
    ///    `Conflict`;
    /// 3. moves every remaining verified transaction back to the unverified
    ///    queue and resets verified-only bookkeeping, matching C#'s
    ///    `InvalidateVerifiedTransactions`.
    ///
    /// Returns the evicted conflict transactions with their removal reasons so
    /// the caller can publish `TransactionRemoved` events.
    pub fn update_pool_for_block_persisted(
        &self,
        block_txs: &[Transaction],
    ) -> Vec<(Transaction, TransactionRemovalReason)> {
        let mut guard = self.inner.write();
        let mut removed = Vec::new();

        // (1) Remove mined transactions and build the conflicts map
        // (Conflicts-attribute target hash -> signers of the persisted txs).
        let mut conflicts: HashMap<UInt256, Vec<UInt160>> = HashMap::new();
        let mut persisted: HashSet<UInt256> = HashSet::with_capacity(block_txs.len());
        for tx in block_txs {
            let Ok(hash) = tx.try_hash() else { continue };
            persisted.insert(hash);
            guard.verification_context.confirm(hash);
            if let Some(item) = guard.verified.remove(&hash) {
                let mined = (*item.transaction).clone();
                guard.conflicts.retain(|_, set| {
                    set.remove(&hash);
                    !set.is_empty()
                });
                guard.context_remove(&mined);
            } else if let Some(item) = guard.unverified.remove(&hash) {
                let mined = (*item.transaction).clone();
                guard.context_remove(&mined);
            }
            let signers: Vec<UInt160> = tx.signers().iter().map(|s| s.account).collect();
            for target in conflict_target_hashes(tx) {
                conflicts
                    .entry(target)
                    .or_default()
                    .extend(signers.iter().copied());
            }
        }

        // (2) Evict verified pooled transactions conflicting with the persisted
        // ones. C# iterates `_sortedTransactions` only; transactions already in
        // `_unverifiedTransactions` remain there until a later reverify pass.
        let candidates: Vec<PoolItem> = guard.verified.iter().cloned().collect();
        for item in candidates {
            let Ok(hash) = item.transaction.try_hash() else {
                continue;
            };
            let tx = &*item.transaction;
            let signer_set: HashSet<UInt160> = tx.signers().iter().map(|s| s.account).collect();
            let named_by_persisted = conflicts
                .get(&hash)
                .is_some_and(|signers| signers.iter().any(|s| signer_set.contains(s)));
            let names_persisted = conflict_target_hashes(tx)
                .iter()
                .any(|target| persisted.contains(target));
            if named_by_persisted || names_persisted {
                if let Some(item) = guard.verified.remove(&hash) {
                    let evicted_tx = (*item.transaction).clone();
                    guard.conflicts.retain(|_, set| {
                        set.remove(&hash);
                        !set.is_empty()
                    });
                    guard.context_remove(&evicted_tx);
                    removed.push((evicted_tx, TransactionRemovalReason::Conflict));
                }
            }
        }

        // (3) Invalidate all remaining verified transactions. They were checked
        // against the previous block state; after a block is persisted C# moves
        // them to the unverified queues and clears verified-only context before
        // the next reverify pass.
        let remaining_verified: Vec<PoolItem> = guard.verified.iter().cloned().collect();
        guard.unverified.reserve(remaining_verified.len());
        for item in remaining_verified {
            guard.unverified.insert(item);
        }
        guard.verified.items.clear();
        guard.verified.hashes.clear();
        guard.conflicts.clear();
        guard.verification_context = TransactionVerificationContext::new();
        guard.sender_fees.clear();
        guard.oracle_responses.clear();

        removed
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
                guard.context_remove(&tx);
                removed.push((tx, TransactionRemovalReason::NoLongerValid));
            }
            if let Some(item) = guard.unverified.remove(hash) {
                let tx = (*item.transaction).clone();
                guard.context_remove(&tx);
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
                guard.context_remove(&tx);
                removals.push((tx, TransactionRemovalReason::NoLongerValid));
            }
        }
        removals
    }

    /// Re-verifies up to `max_count` unverified transactions in priority order.
    ///
    /// Mirrors C# `MemoryPool.ReverifyTransactions`: after a block persist moves
    /// verified survivors into the unverified pool, the blockchain actor promotes
    /// the highest-priority still-valid transactions back into the verified pool,
    /// rebuilding the per-block fee/conflict/oracle bookkeeping as it goes.
    /// Returns whether unverified transactions remain after this pass.
    pub fn reverify_top_unverified(&self, snapshot: &DataCache, max_count: usize) -> bool {
        if max_count == 0 {
            return self.unverified_count() > 0;
        }

        let mut invalid_transactions = Vec::new();
        let more_unverified = {
            let mut guard = self.inner.write();
            let to_check: Vec<PoolItem> = guard
                .unverified
                .to_sorted_vec()
                .into_iter()
                .take(max_count)
                .collect();

            for item in to_check {
                let hash = item.hash();
                if !guard.unverified.contains(&hash) {
                    continue;
                }

                let tx = (*item.transaction).clone();
                let conflicts_to_remove = match guard.check_conflicts(&tx) {
                    Some(conflicts) => conflicts,
                    None => {
                        guard.unverified.remove(&hash);
                        guard.context_remove(&tx);
                        invalid_transactions.push(tx);
                        continue;
                    }
                };

                let pooled_sender_fee = tx
                    .signers()
                    .first()
                    .and_then(|s| guard.sender_fees.get(&s.account).cloned())
                    .unwrap_or_default();
                let tx_sender = tx.signers().first().map(|s| s.account);
                let rebate = conflict_rebate(&conflicts_to_remove, tx_sender);
                let effective_pooled_fee = &pooled_sender_fee - &rebate;
                let oracle_duplicate = oracle_response_id(&tx)
                    .is_some_and(|id| guard.oracle_responses.contains_key(&id));
                let result = crate::verification::verify_transaction(
                    &tx,
                    snapshot,
                    &self.settings,
                    &effective_pooled_fee,
                    oracle_duplicate,
                );

                if result != VerifyResult::Succeed {
                    guard.unverified.remove(&hash);
                    guard.context_remove(&tx);
                    invalid_transactions.push(tx);
                    continue;
                }

                guard.unverified.remove(&hash);
                guard.verified.insert(item);
                guard.context_add(&tx);

                for conflict in &conflicts_to_remove {
                    let conflict_hash = conflict.hash();
                    if let Some(removed) = guard.verified.remove(&conflict_hash) {
                        let dropped = (*removed.transaction).clone();
                        guard.context_remove(&dropped);
                        guard.conflicts.retain(|_, set| {
                            set.remove(&conflict_hash);
                            !set.is_empty()
                        });
                        invalid_transactions.push(dropped);
                    }
                }
                for target in conflict_target_hashes(&tx) {
                    guard.conflicts.entry(target).or_default().insert(hash);
                }
            }

            !guard.unverified.is_empty()
        };

        if !invalid_transactions.is_empty() {
            if let Some(callback) = &self.transaction_removed {
                let args = TransactionRemovedEventArgs::new(
                    invalid_transactions,
                    TransactionRemovalReason::NoLongerValid,
                );
                callback(self, &args);
            }
        }

        more_unverified
    }

    /// Attempts to admit a fresh transaction into the pool. Returns
    /// the [`VerifyResult`] describing the outcome.
    ///
    /// Mirrors C# `MemoryPool.TryAdd`: containment first, then the
    /// full transaction verification ([`crate::verification`] — in C#
    /// the state-independent half runs in the `TransactionRouter`
    /// preverifier and the state-dependent half inside `TryAdd`; the
    /// combined behavior is identical for the single-threaded
    /// admission path), then admission into the **verified** queue
    /// with capacity eviction and verification-context bookkeeping
    /// (sender fees, pooled oracle-response ids).
    ///
    /// Pooled-conflict handling matches C# `CheckConflicts`: a transaction is
    /// rejected (`HasConflicts`) when a conflicting pooled transaction out-fees
    /// it or names a conflictee it shares no signer with; otherwise the
    /// conflicting pooled transactions are evicted on admission, the
    /// conflict-fee rebate is applied to the sender-fee balance check, and the
    /// transaction's own `Conflicts` attributes are tracked for future
    /// admissions. On-chain conflict records are checked separately via the
    /// `Conflicts` attribute verification.
    pub fn try_add(&self, transaction: Transaction, snapshot: &DataCache) -> VerifyResult {
        let hash = transaction.hash();

        // Subscriber veto gate.
        if let Some(callback) = &self.new_transaction {
            let mut args = NewTransactionEventArgs::new(transaction.clone(), snapshot.clone());
            callback(self, &mut args);
            if args.cancel {
                return VerifyResult::PolicyFail;
            }
        }

        // C# TryAdd holds the write lock across the containment check, the
        // sender-fee-context read, verification, and admission, so two
        // concurrent submissions cannot both verify against the same pooled
        // sender-fee state (MemoryPool.cs:353-369). Verification is serialized
        // under the lock exactly like C#'s `_txRwLock.EnterWriteLock()`.
        let (removed_transactions, new_tx_evicted) = {
            let mut guard = self.inner.write();
            if guard.verified.contains(&hash) || guard.unverified.contains(&hash) {
                return VerifyResult::AlreadyInPool;
            }

            // C# CheckConflicts (MemoryPool.cs:330): a transaction that loses the
            // conflict-fee comparison or names a conflictee it shares no signer
            // with is rejected; otherwise the returned pooled txs are evicted
            // once `tx` is admitted.
            let conflicts_to_remove = match guard.check_conflicts(&transaction) {
                Some(list) => list,
                None => return VerifyResult::HasConflicts,
            };

            let pooled_sender_fee = transaction
                .signers()
                .first()
                .and_then(|s| guard.sender_fees.get(&s.account).cloned())
                .unwrap_or_default();
            // Conflict-fee rebate (C# VerifyStateDependent receives conflictsList):
            // the conflicting txs with this sender will be evicted, so their fees
            // no longer count against the sender's pooled-fee allowance.
            let tx_sender = transaction.signers().first().map(|s| s.account);
            let rebate = conflict_rebate(&conflicts_to_remove, tx_sender);
            let effective_pooled_fee = &pooled_sender_fee - &rebate;
            let oracle_duplicate = oracle_response_id(&transaction)
                .is_some_and(|id| guard.oracle_responses.contains_key(&id));

            // Full C# Transaction.Verify against the provided snapshot.
            let result = crate::verification::verify_transaction(
                &transaction,
                snapshot,
                &self.settings,
                &effective_pooled_fee,
                oracle_duplicate,
            );
            if result != VerifyResult::Succeed {
                return result;
            }

            // C# order: add the tx, evict the conflicting pooled txs, record the
            // tx's own Conflicts attributes, then RemoveOverCapacity.
            guard.verified.insert(PoolItem::new(transaction.clone()));
            guard.context_add(&transaction);

            let mut removed_transactions = Vec::new();
            for conflict in &conflicts_to_remove {
                let chash = conflict.hash();
                if let Some(removed) = guard.verified.remove(&chash) {
                    let dropped = (*removed.transaction).clone();
                    guard.context_remove(&dropped);
                    // Drop the evicted tx from every Conflicts tracking set.
                    guard.conflicts.retain(|_, set| {
                        set.remove(&chash);
                        !set.is_empty()
                    });
                    removed_transactions.push(dropped);
                }
            }
            // Track this tx's declared conflicts: target hash -> {tx hash}.
            for target in conflict_target_hashes(&transaction) {
                guard.conflicts.entry(target).or_default().insert(hash);
            }

            // C# RemoveOverCapacity loops over the TOTAL count (verified +
            // unverified), evicting the global lowest-fee item each pass
            // (preferring the unverified queue on a tie) until total <= Capacity.
            // The evicted item may be the just-added transaction => OutOfMemory
            // for the caller. The previous gate only counted the verified queue,
            // so block-persist survivors dumped into the unverified queue could
            // push total occupancy to ~2x the configured capacity.
            let mut new_tx_evicted = false;
            while guard.verified.len() + guard.unverified.len() > guard.capacity {
                let Some(dropped) = guard.remove_lowest_fee() else {
                    break;
                };
                if dropped.hash() == hash {
                    new_tx_evicted = true;
                }
                removed_transactions.push(dropped);
            }
            (removed_transactions, new_tx_evicted)
        };

        if let Some(callback) = &self.transaction_added {
            callback(self, &transaction);
        }
        if !removed_transactions.is_empty() {
            if let Some(callback) = &self.transaction_removed {
                let args = TransactionRemovedEventArgs::new(
                    removed_transactions,
                    TransactionRemovalReason::CapacityExceeded,
                );
                callback(self, &args);
            }
        }
        if new_tx_evicted {
            return VerifyResult::OutOfMemory;
        }
        VerifyResult::Succeed
    }

    /// Removes the transaction with the given hash from the pool
    /// and emits the `transaction_removed` event.
    pub fn remove(&self, hash: &UInt256, reason: TransactionRemovalReason) {
        let tx_opt = {
            let mut guard = self.inner.write();
            let removed = guard
                .verified
                .remove(hash)
                .or_else(|| guard.unverified.remove(hash))
                .map(|item| (*item.transaction).clone());
            if let Some(tx) = &removed {
                guard.context_remove(tx);
            }
            removed
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

/// Shared handle alias for the `Arc<MemoryPool>` pattern used by
/// services that need to share the pool across tasks.
pub type SharedMemoryPool = Arc<MemoryPool>;

#[cfg(test)]
mod tests;

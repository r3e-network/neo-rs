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
    /// 2. evicts pooled transactions that conflict with the persisted ones, in
    ///    both directions — a pooled tx whose hash is named by a persisted tx's
    ///    `Conflicts` attribute (with an intersecting signer), or whose own
    ///    `Conflicts` attribute names a persisted tx — fired as `Conflict`;
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

        // (2) Evict pooled transactions conflicting with the persisted ones.
        let candidates: Vec<PoolItem> = guard
            .verified
            .iter()
            .chain(guard.unverified.iter())
            .cloned()
            .collect();
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
                let evicted = guard
                    .verified
                    .remove(&hash)
                    .or_else(|| guard.unverified.remove(&hash));
                if let Some(item) = evicted {
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
                return VerifyResult::AlreadyExists;
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
            // the conflicting txs sharing this sender will be evicted, so their
            // fees no longer count against the sender's pooled-fee allowance.
            let tx_sender = transaction.signers().first().map(|s| s.account);
            let rebate: BigInt = conflicts_to_remove
                .iter()
                .filter(|c| {
                    tx_sender
                        .is_some_and(|s| c.transaction.signers().iter().any(|sig| sig.account == s))
                })
                .map(|c| {
                    BigInt::from(c.transaction.system_fee())
                        + BigInt::from(c.transaction.network_fee())
                })
                .sum();
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::signature::Secp256r1Crypto;
    use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
    use neo_primitives::{UInt160, WitnessScope};
    use neo_serialization::BinarySerializer;
    use neo_vm::StackItem;
    use neo_vm_rs::{ExecutionEngineLimits, OpCode};

    /// Deterministic secp256r1 keypair: (private key, SEC1 pubkey,
    /// signature-contract script hash).
    fn keypair(seed: u8) -> ([u8; 32], Vec<u8>, UInt160) {
        let private = [seed; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let script = neo_vm::script_builder::redeem_script::signature_redeem_script(&public);
        (private, public, UInt160::from_script(&script))
    }

    /// Writes a GAS NEP-17 account record (`Struct[balance]`, the C#
    /// `FungibleToken.AccountState`) so the verification balance check
    /// passes.
    fn mint_gas(snapshot: &DataCache, account: &UInt160, datoshi: i64) {
        let item =
            StackItem::from_struct(vec![StackItem::from_int(num_bigint::BigInt::from(datoshi))]);
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
        let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
        key.extend_from_slice(&account.to_bytes());
        snapshot.add(
            neo_storage::StorageKey::new(neo_native_contracts::GasToken::ID, key),
            neo_storage::StorageItem::from_bytes(bytes),
        );
    }

    /// Builds a properly signed standard single-signature transaction.
    fn signed_tx(
        settings: &ProtocolSettings,
        private: &[u8; 32],
        public: &[u8],
        account: UInt160,
        nonce: u32,
        valid_until_block: u32,
        attributes: Vec<TransactionAttribute>,
    ) -> Transaction {
        signed_tx_with_fees(
            settings,
            private,
            public,
            account,
            nonce,
            valid_until_block,
            100,
            3_000_000,
            attributes,
        )
    }

    fn signed_tx_with_fees(
        settings: &ProtocolSettings,
        private: &[u8; 32],
        public: &[u8],
        account: UInt160,
        nonce: u32,
        valid_until_block: u32,
        system_fee: i64,
        network_fee: i64,
        attributes: Vec<TransactionAttribute>,
    ) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_system_fee(system_fee);
        tx.set_network_fee(network_fee); // covers size fee + sig-check cost
        tx.set_valid_until_block(valid_until_block);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_attributes(attributes);
        tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

        // Sign data = network magic (u32 LE) ‖ tx hash.
        let hash = tx.try_hash().expect("tx hash");
        let mut data = settings.network.to_le_bytes().to_vec();
        data.extend_from_slice(&hash.to_bytes());
        let signature = Secp256r1Crypto::sign(&data, private).expect("sign");

        let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        let verification = neo_vm::script_builder::redeem_script::signature_redeem_script(public);
        tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
        tx
    }

    /// (settings, snapshot-with-funds, keypair) fixture.
    fn fixture(seed: u8) -> (ProtocolSettings, DataCache, [u8; 32], Vec<u8>, UInt160) {
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let (private, public, account) = keypair(seed);
        mint_gas(&snapshot, &account, 100_000_000); // 1 GAS
        (settings, snapshot, private, public, account)
    }

    #[test]
    fn empty_pool_has_zero_counts() {
        let pool = MemoryPool::new(&ProtocolSettings::default());
        assert_eq!(pool.total_count(), 0);
        assert_eq!(pool.verified_count(), 0);
        assert_eq!(pool.unverified_count(), 0);
    }

    #[test]
    fn valid_signed_transaction_is_admitted_verified() {
        let (settings, snapshot, private, public, account) = fixture(0x42);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 1, 1, Vec::new());
        let hash = tx.hash();
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        assert_eq!(
            pool.verified_count(),
            1,
            "C# TryAdd admits into the sorted pool"
        );
        assert_eq!(pool.unverified_count(), 0);
        assert!(pool.contains(&hash));
    }

    #[test]
    fn block_persist_removes_mined_tx_and_evicts_conflicts() {
        let (settings, snapshot, private, public, account) = fixture(0x55);
        let pool = MemoryPool::new(&settings);

        // `mined` is pooled and will be in the block (leg 1: removed).
        let mined = signed_tx(&settings, &private, &public, account, 10, 100, Vec::new());
        // `target` is NOT pooled, only in the block; `conflicting` (pooled,
        // same signer) names it as a Conflicts target (leg 2: evicted on
        // persist because its conflict target becomes on-chain).
        let target = signed_tx(&settings, &private, &public, account, 12, 100, Vec::new());
        let conflicting = signed_tx(
            &settings,
            &private,
            &public,
            account,
            11,
            100,
            vec![TransactionAttribute::Conflicts(
                neo_payloads::Conflicts::new(target.hash()),
            )],
        );
        assert_eq!(
            pool.try_add(mined.clone(), &snapshot),
            VerifyResult::Succeed
        );
        assert_eq!(
            pool.try_add(conflicting.clone(), &snapshot),
            VerifyResult::Succeed
        );
        assert_eq!(pool.verified_count(), 2);

        // Persist a block containing `mined` and `target`. C#
        // UpdatePoolForBlockPersisted: `mined` is removed (it was confirmed),
        // and `conflicting` is evicted because its Conflicts attribute names the
        // now-persisted `target`.
        let removed = pool.update_pool_for_block_persisted(&[mined.clone(), target.clone()]);
        assert_eq!(
            pool.verified_count(),
            0,
            "both the mined tx and its conflict leave the pool"
        );
        assert!(!pool.contains(&mined.hash()));
        assert!(!pool.contains(&conflicting.hash()));
        assert!(
            removed
                .iter()
                .any(|(tx, reason)| tx.hash() == conflicting.hash()
                    && *reason == TransactionRemovalReason::Conflict),
            "the conflicting tx is reported as a Conflict removal"
        );
    }

    #[test]
    fn block_persist_invalidates_remaining_verified_transactions() {
        let (settings, snapshot, private, public, account) = fixture(0x4E);
        let pool = MemoryPool::new(&settings);
        let first = signed_tx(&settings, &private, &public, account, 20, 1, Vec::new());
        let second = signed_tx(&settings, &private, &public, account, 21, 1, Vec::new());

        assert_eq!(
            pool.try_add(first.clone(), &snapshot),
            VerifyResult::Succeed
        );
        assert_eq!(
            pool.try_add(second.clone(), &snapshot),
            VerifyResult::Succeed
        );
        assert_eq!(pool.verified_count(), 2);
        assert_eq!(pool.unverified_count(), 0);

        let removed = pool.update_pool_for_block_persisted(&[]);
        assert!(removed.is_empty());

        let verified: HashSet<UInt256> = pool
            .verified_snapshot()
            .into_iter()
            .map(|item| item.hash())
            .collect();
        let unverified: HashSet<UInt256> = pool
            .unverified_snapshot()
            .into_iter()
            .map(|item| item.hash())
            .collect();
        assert!(verified.is_empty());
        assert!(unverified.contains(&first.hash()));
        assert!(unverified.contains(&second.hash()));
        assert_eq!(pool.verified_count(), 0);
        assert_eq!(pool.unverified_count(), 2);
    }

    #[test]
    fn verified_lookup_does_not_return_unverified_transactions() {
        let (settings, snapshot, private, public, account) = fixture(0x4F);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 22, 1, Vec::new());
        let hash = tx.hash();

        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        assert!(pool.get_verified(&hash).is_some());

        let removed = pool.update_pool_for_block_persisted(&[]);
        assert!(removed.is_empty());
        assert!(pool.get(&hash).is_some());
        assert!(pool.get_verified(&hash).is_none());
    }

    #[test]
    fn duplicate_admission_reports_already_exists() {
        let (settings, snapshot, private, public, account) = fixture(0x43);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 2, 1, Vec::new());
        assert_eq!(pool.try_add(tx.clone(), &snapshot), VerifyResult::Succeed);
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::AlreadyExists);
    }

    #[test]
    fn try_add_conflict_eviction_reports_capacity_exceeded_like_csharp() {
        let (settings, snapshot, private, public, account) = fixture(0x50);
        let mut pool = MemoryPool::new(&settings);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            TransactionRemovalReason,
            Vec<UInt256>,
        )>::new()));
        let captured = events.clone();
        pool.transaction_removed = Some(Box::new(move |_pool, args| {
            captured.lock().unwrap().push((
                args.reason,
                args.transactions.iter().map(|tx| tx.hash()).collect(),
            ));
        }));

        let old = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            30,
            100,
            100,
            3_000_000,
            Vec::new(),
        );
        let replacement = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            31,
            100,
            100,
            6_000_000,
            vec![TransactionAttribute::Conflicts(
                neo_payloads::Conflicts::new(old.hash()),
            )],
        );

        assert_eq!(pool.try_add(old.clone(), &snapshot), VerifyResult::Succeed);
        assert_eq!(
            pool.try_add(replacement.clone(), &snapshot),
            VerifyResult::Succeed
        );

        assert!(!pool.contains(&old.hash()));
        assert!(pool.contains(&replacement.hash()));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, TransactionRemovalReason::CapacityExceeded);
        assert_eq!(events[0].1, vec![old.hash()]);
    }

    #[test]
    fn try_add_self_capacity_eviction_fires_added_then_removed_before_out_of_memory() {
        let (mut settings, snapshot, private, public, account) = fixture(0x51);
        settings.memory_pool_max_transactions = 1;
        let mut pool = MemoryPool::new(&settings);
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let added_calls = calls.clone();
        pool.transaction_added = Some(Box::new(move |_pool, tx| {
            added_calls
                .lock()
                .unwrap()
                .push(format!("added:{}", tx.hash()));
        }));
        let removed_calls = calls.clone();
        pool.transaction_removed = Some(Box::new(move |_pool, args| {
            removed_calls.lock().unwrap().push(format!(
                "removed:{:?}:{:?}",
                args.reason,
                args.transactions
                    .iter()
                    .map(|tx| tx.hash())
                    .collect::<Vec<_>>()
            ));
        }));

        let kept = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            32,
            100,
            100,
            6_000_000,
            Vec::new(),
        );
        let evicted = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            33,
            100,
            100,
            3_000_000,
            Vec::new(),
        );

        assert_eq!(pool.try_add(kept.clone(), &snapshot), VerifyResult::Succeed);
        assert_eq!(
            pool.try_add(evicted.clone(), &snapshot),
            VerifyResult::OutOfMemory
        );

        assert!(pool.contains(&kept.hash()));
        assert!(!pool.contains(&evicted.hash()));
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            [
                format!("added:{}", kept.hash()),
                format!("added:{}", evicted.hash()),
                format!(
                    "removed:{:?}:{:?}",
                    TransactionRemovalReason::CapacityExceeded,
                    vec![evicted.hash()]
                ),
            ]
        );
    }

    #[test]
    fn tampered_signature_reports_invalid_signature() {
        let (settings, snapshot, private, public, account) = fixture(0x44);
        let pool = MemoryPool::new(&settings);
        let mut tx = signed_tx(&settings, &private, &public, account, 3, 1, Vec::new());
        let mut witnesses = tx.witnesses().to_vec();
        *witnesses[0].invocation_script.last_mut().unwrap() ^= 0x01;
        tx.set_witnesses(witnesses);
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidSignature);
    }

    #[test]
    fn expired_transaction_reports_expired() {
        let (settings, snapshot, private, public, account) = fixture(0x45);
        let pool = MemoryPool::new(&settings);
        // C# VerifyStateDependent: ValidUntilBlock <= height (0) → Expired.
        let tx = signed_tx(&settings, &private, &public, account, 4, 0, Vec::new());
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Expired);
    }

    #[test]
    fn bad_script_reports_invalid_script() {
        let (settings, snapshot, private, public, account) = fixture(0x46);
        let pool = MemoryPool::new(&settings);
        let mut tx = signed_tx(&settings, &private, &public, account, 5, 1, Vec::new());
        tx.set_script(vec![0xff]); // reserved opcode → strict parse failure
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidScript);
    }

    #[test]
    fn oversize_transaction_reports_oversize() {
        let (settings, snapshot, private, public, account) = fixture(0x47);
        let pool = MemoryPool::new(&settings);
        let mut tx = signed_tx(&settings, &private, &public, account, 6, 1, Vec::new());
        tx.set_script(vec![
            OpCode::PUSH1.byte();
            neo_payloads::MAX_TRANSACTION_SIZE
        ]);
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::OverSize);
    }

    #[test]
    fn blocked_sender_reports_policy_fail() {
        let (settings, snapshot, private, public, account) = fixture(0x48);
        // PolicyContract Prefix_BlockedAccount (15) + account.
        let mut key = vec![15u8];
        key.extend_from_slice(&account.to_bytes());
        snapshot.add(
            neo_storage::StorageKey::new(-7, key),
            neo_storage::StorageItem::from_bytes(Vec::new()),
        );
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 7, 1, Vec::new());
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::PolicyFail);
    }

    #[test]
    fn missing_balance_reports_insufficient_funds() {
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false); // no GAS minted
        let (private, public, account) = keypair(0x49);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 8, 1, Vec::new());
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InsufficientFunds);
    }

    #[test]
    fn not_valid_before_reports_invalid_attribute() {
        let (settings, snapshot, private, public, account) = fixture(0x4A);
        let pool = MemoryPool::new(&settings);
        // NotValidBefore(5) at height 0 → C# NotValidBefore.Verify false.
        let attributes = vec![TransactionAttribute::not_valid_before(5)];
        let tx = signed_tx(&settings, &private, &public, account, 9, 1, attributes);
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidAttribute);
    }

    #[test]
    fn sender_fee_accumulates_until_balance_exhausted() {
        let (settings, snapshot, private, public, account) = fixture(0x4B);
        let pool = MemoryPool::new(&settings);
        // Each tx charges 100 + 3_000_000 against the 100M-datoshi balance.
        // Shrink the balance so only one fits: 2 × 3_000_100 > 4_000_000.
        let mut key = vec![20u8];
        key.extend_from_slice(&account.to_bytes());
        snapshot.delete(&neo_storage::StorageKey::new(
            neo_native_contracts::GasToken::ID,
            key,
        ));
        mint_gas(&snapshot, &account, 4_000_000);
        let first = signed_tx(&settings, &private, &public, account, 10, 1, Vec::new());
        let second = signed_tx(&settings, &private, &public, account, 11, 1, Vec::new());
        assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
        assert_eq!(
            pool.try_add(second, &snapshot),
            VerifyResult::InsufficientFunds,
            "pooled sender fees must count against the balance (C# senderFee)"
        );
    }

    #[test]
    fn commit_block_removes_confirmed_and_releases_sender_fee() {
        let (settings, snapshot, private, public, account) = fixture(0x4C);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 12, 1, Vec::new());
        let hash = tx.hash();
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);

        let removed = pool.commit_block(&[hash]);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].1, TransactionRemovalReason::NoLongerValid);
        assert!(!pool.contains(&hash));

        // The sender-fee reservation is released: a fresh tx fits again.
        let next = signed_tx(&settings, &private, &public, account, 13, 1, Vec::new());
        assert_eq!(pool.try_add(next, &snapshot), VerifyResult::Succeed);
    }

    #[test]
    fn reverify_with_empty_unverified_is_noop() {
        let (settings, snapshot, private, public, account) = fixture(0x4D);
        let pool = MemoryPool::new(&settings);
        let tx = signed_tx(&settings, &private, &public, account, 14, 1, Vec::new());
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        // try_add admits straight into the verified queue (C# TryAdd), so
        // there is nothing to promote.
        let removals = pool.reverify(&snapshot, |_tx, _snap| VerifyResult::Succeed);
        assert!(removals.is_empty());
        assert_eq!(pool.verified_count(), 1);
        assert_eq!(pool.unverified_count(), 0);
    }
}

/// Shared handle alias for the `Arc<MemoryPool>` pattern used by
/// services that need to share the pool across tasks.
pub type SharedMemoryPool = Arc<MemoryPool>;

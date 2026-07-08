//! Private memory-pool queue and verification-context state.
//!
//! The public `MemoryPool` facade owns admission, callbacks, and event
//! ordering. This module owns the mutable queue indexes and C# parity helpers
//! used while the pool write lock is held.

use std::collections::{HashMap, HashSet};

use num_bigint::BigInt;

use crate::pool_index::PoolIndex;
use crate::pool_item::PoolItem;
use crate::transaction_verification_context::TransactionVerificationContext;
use neo_payloads::Transaction;
use neo_primitives::{UInt160, UInt256};

/// Inner, mutable state of the memory pool. Split out so the outer
/// `MemoryPool` can hand out read-only references while still
/// allowing the rest of the system to mutate the pool under a lock.
pub(super) struct MemoryPoolInner {
    pub(super) verified: PoolIndex,
    pub(super) unverified: PoolIndex,
    pub(super) conflicts: HashMap<UInt256, HashSet<UInt256>>,
    pub(super) verification_context: TransactionVerificationContext,
    /// C# `TransactionVerificationContext._senderFee`: the summed
    /// system+network fees of pooled transactions per sender, charged
    /// against the sender's GAS balance for every new admission.
    pub(super) sender_fees: HashMap<UInt160, BigInt>,
    /// C# `TransactionVerificationContext._oracleResponses`: pooled
    /// `OracleResponse` ids, rejecting duplicate responses.
    pub(super) oracle_responses: HashMap<u64, UInt256>,
    pub(super) capacity: usize,
}

impl MemoryPoolInner {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            verified: PoolIndex::with_capacity(capacity),
            unverified: PoolIndex::with_capacity(capacity / 4),
            conflicts: HashMap::with_capacity(capacity / 2),
            verification_context: TransactionVerificationContext::new(),
            sender_fees: HashMap::new(),
            oracle_responses: HashMap::new(),
            capacity,
        }
    }

    /// C# `TransactionVerificationContext.AddTransaction`.
    pub(super) fn context_add(&mut self, tx: &Transaction) {
        if let Some(oracle) = oracle_response_id(tx) {
            self.oracle_responses.insert(oracle, tx.hash());
        }
        if let Some(sender) = tx.signers().first().map(|s| s.account) {
            let fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
            *self.sender_fees.entry(sender).or_default() += fee;
        }
    }

    /// C# `TransactionVerificationContext.RemoveTransaction`.
    pub(super) fn context_remove(&mut self, tx: &Transaction) {
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
    /// pooled transactions out-fee `tx` (sum of their network fees >= `tx`'s).
    pub(super) fn check_conflicts(&self, tx: &Transaction) -> Option<Vec<PoolItem>> {
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
    pub(super) fn remove_lowest_fee(&mut self) -> Option<Transaction> {
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
        // C# RemoveOverCapacity gates the verification-context + conflict cleanup
        // on `ReferenceEquals(sortedPool, _sortedTransactions)` — i.e. it runs only
        // for a verified-queue eviction. Unverified items are never tracked in
        // sender_fees / conflicts (those maps are cleared on block-persist and only
        // repopulated for verified admissions), so this is a no-op for them; gating
        // it mirrors C# exactly and documents the invariant.
        if !from_unverified {
            self.context_remove(&dropped);
            self.conflicts.retain(|_, set| {
                set.remove(&hash);
                !set.is_empty()
            });
        }
        Some(dropped)
    }
}

/// The hashes a transaction declares as conflicting via its `Conflicts` attributes.
pub(super) fn conflict_target_hashes(tx: &Transaction) -> Vec<UInt256> {
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
pub(super) fn conflict_rebate(conflicts: &[PoolItem], tx_sender: Option<UInt160>) -> BigInt {
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
pub(super) fn oracle_response_id(tx: &Transaction) -> Option<u64> {
    tx.attributes().iter().find_map(|attr| match attr {
        neo_payloads::TransactionAttribute::OracleResponse(resp) => Some(resp.id),
        _ => None,
    })
}

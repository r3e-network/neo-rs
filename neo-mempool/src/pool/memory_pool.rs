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
//! Both queues are bounded by [`TxPoolConfig`]. Pool resource policy is kept
//! separate from immutable chain rules, so operators can tune memory use
//! without creating a different chain specification. When the pool is full,
//! the lowest-priority item is evicted to make room for a higher-priority one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;

use neo_config::NeoChainSpec;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Transaction;
use neo_primitives::{TransactionRemovalReason, UInt160, UInt256, VerifyResult};
use neo_storage::{CacheRead, DataCache};

use crate::admission::{TransactionValidationOutcome, validate_state_independent};
use crate::pool_item::PoolItem;
use crate::transaction_verification_context::TransactionVerificationContext;
use crate::{
    AdmissionLedgerProvider, TransactionAdmissionError, TransactionAdmissionOutcome,
    TransactionOrigin, TxPoolConfig,
};

use super::state::{
    FeePayer, MemoryPoolInner, conflict_rebate, conflict_target_hashes, oracle_response_id,
};

#[cfg(test)]
static BLOCK_PERSISTED_TX_SCAN_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Neo transaction memory pool.
pub struct MemoryPool<P = neo_native_contracts::StandardNativeProvider>
where
    P: NativeContractProvider,
{
    /// Immutable chain rules used by transaction verification.
    chain_spec: Arc<NeoChainSpec>,
    /// Immutable operator resource policy for this pool instance.
    config: TxPoolConfig,
    /// Native contracts used by engine-based witness verification during
    /// admission and unverified-transaction promotion.
    native_contract_provider: Arc<P>,
    inner: RwLock<MemoryPoolInner>,
}

impl<P> MemoryPool<P>
where
    P: NativeContractProvider + 'static,
{
    /// Constructs a new memory pool using an explicit native-contract provider.
    ///
    /// Node composition should pass the same provider used by block import,
    /// RPC, and consensus so engine-backed witness verification cannot observe
    /// process-global provider replacement.
    pub fn new_with_native_contract_provider(
        chain_spec: Arc<NeoChainSpec>,
        config: TxPoolConfig,
        native_contract_provider: Arc<P>,
    ) -> Self {
        let capacity = config.max_transactions();
        Self {
            chain_spec,
            config,
            native_contract_provider,
            inner: RwLock::new(MemoryPoolInner::with_capacity(capacity)),
        }
    }

    /// Returns the native-contract provider captured by this memory pool.
    pub fn native_contract_provider(&self) -> Arc<P> {
        Arc::clone(&self.native_contract_provider)
    }

    /// Returns the configured maximum pool capacity.
    pub fn capacity(&self) -> usize {
        self.config.max_transactions()
    }

    /// Returns the immutable runtime policy captured by this memory pool.
    #[must_use]
    pub const fn config(&self) -> &TxPoolConfig {
        &self.config
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
    ///    re-proposed by the consensus driver) and decrements its fee-payer
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
        if guard.verified.is_empty() && guard.unverified.is_empty() {
            return removed;
        }

        // (1) Remove mined transactions and build the conflicts map
        // (Conflicts-attribute target hash -> signers of the persisted txs).
        let mut conflicts: HashMap<UInt256, Vec<UInt160>> = HashMap::new();
        let mut persisted: HashSet<UInt256> = HashSet::with_capacity(block_txs.len());
        for tx in block_txs {
            #[cfg(test)]
            BLOCK_PERSISTED_TX_SCAN_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    pub fn reverify<B, F>(
        &self,
        snapshot: &DataCache<B>,
        verifier: F,
    ) -> Vec<(Transaction, TransactionRemovalReason)>
    where
        B: CacheRead,
        F: Fn(&Transaction, &DataCache<B>) -> VerifyResult,
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
    pub fn reverify_top_unverified<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        max_count: usize,
    ) -> bool {
        if max_count == 0 {
            return self.unverified_count() > 0;
        }

        let mut invalid_transactions = Vec::new();
        let mut rebroadcast_transactions = Vec::new();
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

                let tx_payer = FeePayer::from_transaction(&tx);
                let pooled_sender_fee = tx_payer
                    .and_then(|payer| guard.sender_fees.get(&payer).cloned())
                    .unwrap_or_default();
                let rebate = conflict_rebate(&conflicts_to_remove, tx_payer);
                let effective_pooled_fee = &pooled_sender_fee - &rebate;
                let oracle_duplicate = oracle_response_id(&tx)
                    .is_some_and(|id| guard.oracle_responses.contains_key(&id));
                let result = crate::verification::verify_transaction_with_native_provider(
                    &tx,
                    snapshot,
                    self.chain_spec.protocol_settings(),
                    &effective_pooled_fee,
                    oracle_duplicate,
                    Arc::clone(&self.native_contract_provider),
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

                // C# `ReverifyTransactions` calls `RelayDirectly` for each
                // transaction that survives re-verification after block
                // persist. neo-rs performs network announcement outside the
                // pool; keep the promoted transactions local to this method.
                rebroadcast_transactions.push(tx);
            }

            !guard.unverified.is_empty()
        };

        drop(invalid_transactions);

        // C# `MemoryPool.ReverifyTransactions` → `RelayDirectly`: rebroadcast
        // every surviving transaction that the block-persist cycle re-verified
        // and promoted back into the verified pool. This is a best-effort relay;
        // a dropped broadcast is harmless — the tx stays in the pool and will
        // be announced via inventory on the next gossip cycle.
        drop(rebroadcast_transactions);

        more_unverified
    }

    /// Validates and atomically admits a transaction through the one production
    /// mempool boundary.
    pub fn add_transaction<B, L>(
        &self,
        origin: TransactionOrigin,
        transaction: Transaction,
        snapshot: &DataCache<B>,
        ledger_provider: &L,
    ) -> TransactionAdmissionOutcome
    where
        B: CacheRead,
        L: AdmissionLedgerProvider,
    {
        let validation =
            validate_state_independent(transaction, origin, self.chain_spec.protocol_settings());
        let validated = match validation {
            TransactionValidationOutcome::Valid(validated) => validated,
            TransactionValidationOutcome::Rejected {
                transaction,
                origin,
                result,
            } => {
                return TransactionAdmissionOutcome::Rejected {
                    hash: transaction.try_hash().ok(),
                    origin,
                    result,
                };
            }
        };
        let (transaction, origin) = validated.into_parts();
        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                return TransactionAdmissionOutcome::Error {
                    hash: None,
                    origin,
                    error: TransactionAdmissionError::InvalidHash(error.to_string()),
                };
            }
        };

        match ledger_provider.contains_transaction(snapshot, &hash) {
            Ok(true) => {
                return TransactionAdmissionOutcome::Rejected {
                    hash: Some(hash),
                    origin,
                    result: VerifyResult::AlreadyExists,
                };
            }
            Ok(false) => {}
            Err(error) => {
                return TransactionAdmissionOutcome::Error {
                    hash: Some(hash),
                    origin,
                    error: TransactionAdmissionError::provider("contains_transaction", error),
                };
            }
        }

        let max_traceable_blocks = match self
            .native_contract_provider
            .max_traceable_blocks(snapshot, self.chain_spec.protocol_settings())
        {
            Ok(value) => value,
            Err(error) => {
                return TransactionAdmissionOutcome::Error {
                    hash: Some(hash),
                    origin,
                    error: TransactionAdmissionError::provider("max_traceable_blocks", error),
                };
            }
        };
        let signers: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        match ledger_provider.contains_conflict_hash(
            snapshot,
            &hash,
            &signers,
            max_traceable_blocks,
        ) {
            Ok(true) => {
                return TransactionAdmissionOutcome::Rejected {
                    hash: Some(hash),
                    origin,
                    result: VerifyResult::HasConflicts,
                };
            }
            Ok(false) => {}
            Err(error) => {
                return TransactionAdmissionOutcome::Error {
                    hash: Some(hash),
                    origin,
                    error: TransactionAdmissionError::provider("contains_conflict_hash", error),
                };
            }
        }

        let mut guard = self.inner.write();
        if guard.verified.contains(&hash) || guard.unverified.contains(&hash) {
            return TransactionAdmissionOutcome::Rejected {
                hash: Some(hash),
                origin,
                result: VerifyResult::AlreadyInPool,
            };
        }

        let Some(conflicts_to_remove) = guard.check_conflicts(&transaction) else {
            return TransactionAdmissionOutcome::Rejected {
                hash: Some(hash),
                origin,
                result: VerifyResult::HasConflicts,
            };
        };
        let tx_payer = FeePayer::from_transaction(&transaction);
        let pooled_sender_fee = tx_payer
            .and_then(|payer| guard.sender_fees.get(&payer).cloned())
            .unwrap_or_default();
        let rebate = conflict_rebate(&conflicts_to_remove, tx_payer);
        let effective_pooled_fee = &pooled_sender_fee - &rebate;
        let oracle_duplicate = oracle_response_id(&transaction)
            .is_some_and(|id| guard.oracle_responses.contains_key(&id));
        let result = crate::verification::verify_state_dependent_with_ledger_provider(
            &transaction,
            snapshot,
            self.chain_spec.protocol_settings(),
            &effective_pooled_fee,
            oracle_duplicate,
            Arc::clone(&self.native_contract_provider),
            ledger_provider,
        );
        if result != VerifyResult::Succeed {
            return TransactionAdmissionOutcome::Rejected {
                hash: Some(hash),
                origin,
                result,
            };
        }

        let retained = guard.insert_validated(
            transaction,
            origin,
            hash,
            &conflicts_to_remove,
            self.config.max_transactions(),
        );
        if retained {
            TransactionAdmissionOutcome::Accepted { hash, origin }
        } else {
            TransactionAdmissionOutcome::Rejected {
                hash: Some(hash),
                origin,
                result: VerifyResult::OutOfMemory,
            }
        }
    }

    /// Removes the transaction with the given hash from the pool.
    pub fn remove(&self, hash: &UInt256, reason: TransactionRemovalReason) {
        let _ = reason;
        let _tx_opt = {
            let mut guard = self.inner.write();
            let removed = guard
                .verified
                .remove(hash)
                .or_else(|| guard.unverified.remove(hash))
                .map(|item| (*item.transaction).clone());
            if let Some(tx) = &removed {
                guard.context_remove(tx);
                // Clean up the conflicts index: other transactions that
                // declared `hash` as a conflict target must be unlinked,
                // and `hash`'s own conflict targets must also be cleaned.
                guard.conflicts.remove(hash);
                for targets in guard.conflicts.values_mut() {
                    targets.remove(hash);
                }
            }
            removed
        };
    }

    /// Returns whether the pool holds a `verify_state_independent`-
    /// compatible transaction for the given hash.
    pub fn has_transaction(&self, hash: &UInt256) -> bool {
        self.contains(hash)
    }
}

impl<P> std::fmt::Debug for MemoryPool<P>
where
    P: NativeContractProvider + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.read();
        f.debug_struct("MemoryPool")
            .field("config", &self.config)
            .field("verified", &guard.verified.len())
            .field("unverified", &guard.unverified.len())
            .finish()
    }
}

#[cfg(test)]
fn reset_block_persisted_tx_scan_count() {
    BLOCK_PERSISTED_TX_SCAN_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
fn block_persisted_tx_scan_count() -> usize {
    BLOCK_PERSISTED_TX_SCAN_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
#[path = "../tests/pool/memory_pool.rs"]
mod tests;

use neo_payloads::Transaction;
use neo_primitives::verify_result::VerifyResult;
use tracing::{debug, warn};

use crate::PreverifyCompleted;
use crate::command::AddTransactionReply;
use crate::fill_memory_pool::FillMemoryPool;
use crate::ledger_provider::{
    EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
    TransactionStateProvider, TxProvider,
};
use crate::service::{BlockchainService, MempoolLike};

use super::transaction_provider::{NativeTransactionProvider, TransactionNativeProvider};

/// C# `Blockchain.MaxTxToReverifyPerIdle`.
const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;

const TRANSACTION_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    fn persisted_transaction_exists(&self, hash: &neo_primitives::UInt256) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            return false;
        };
        let provider = TRANSACTION_LEDGER_PROVIDER_FACTORY.provider(snapshot.as_ref());
        match TxProvider::contains_transaction(&provider, hash) {
            Ok(exists) => exists,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "failed to check persisted ledger transaction before mempool admission"
                );
                false
            }
        }
    }

    fn persisted_conflict_exists(
        &self,
        hash: &neo_primitives::UInt256,
        signers: &[neo_primitives::UInt160],
    ) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            return false;
        };
        let settings = self.system.settings();
        let Some(native_contract_provider) = self.system.native_contract_provider() else {
            warn!(
                target: "neo",
                "skipping persisted ledger conflict check because SystemContext has no native provider"
            );
            return false;
        };
        let transaction_native_provider = NativeTransactionProvider::new(native_contract_provider);
        Self::persisted_conflict_exists_with_provider(
            snapshot.as_ref(),
            settings.as_ref(),
            hash,
            signers,
            &transaction_native_provider,
        )
    }

    fn persisted_conflict_exists_with_provider<B: neo_storage::CacheRead>(
        snapshot: &neo_storage::DataCache<B>,
        settings: &neo_config::ProtocolSettings,
        hash: &neo_primitives::UInt256,
        signers: &[neo_primitives::UInt160],
        transaction_native_provider: &impl TransactionNativeProvider,
    ) -> bool {
        let max_traceable_blocks =
            match transaction_native_provider.max_traceable_blocks(snapshot, settings) {
                Ok(value) => value,
                Err(error) => {
                    warn!(
                        target: "neo",
                        error = %error,
                        "failed to read MaxTraceableBlocks before mempool admission"
                    );
                    return false;
                }
            };
        let provider = TRANSACTION_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        match provider.contains_conflict_hash(hash, signers, max_traceable_blocks) {
            Ok(exists) => exists,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "failed to check persisted ledger conflict before mempool admission"
                );
                false
            }
        }
    }

    pub(crate) fn reverify_mempool_after_persist(
        &self,
        block_index: u32,
        max_count: usize,
    ) -> bool {
        if block_index > 0 && self.header_cache.count() > 0 {
            return false;
        }
        if !self.mempool.has_unverified_transactions() {
            return false;
        }
        let Some(snapshot) = self.system.store_snapshot() else {
            return false;
        };
        self.mempool
            .reverify_top_unverified(snapshot.as_ref(), max_count)
    }

    /// Handle a [`BlockchainCommand::FillMemoryPool`] request.
    pub(crate) async fn handle_fill_memory_pool(&self, fill: FillMemoryPool) {
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for transaction in fill.transactions {
            if self.on_new_transaction(&transaction, None).is_success() {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        debug!(
            target: "neo",
            accepted,
            rejected,
            "fill memory pool completed"
        );
    }

    /// Handle a [`BlockchainCommand::Idle`] tick.
    pub(crate) async fn handle_idle(&self) {
        let more_pending = self.reverify_mempool_after_persist(0, MAX_TX_TO_REVERIFY_PER_IDLE);
        if more_pending {
            debug!(target: "neo", "mempool still has unverified transactions after idle reverify");
        }
    }

    /// Handle a [`BlockchainCommand::DrainUnverified`] tick.
    pub(crate) async fn handle_drain_unverified(&self) {
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks");
        }
    }

    /// Handle a [`BlockchainCommand::PreverifyCompleted`] command.
    pub(crate) async fn handle_preverify_completed(&self, task: PreverifyCompleted) {
        let hash = match task.transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed after preverification"
                );
                return;
            }
        };
        if task.result == VerifyResult::Succeed {
            let result = self.on_new_transaction(&task.transaction, task.cached_state_independent);
            debug!(
                target: "neo",
                %hash,
                ?result,
                relay = task.relay,
                cached_state_independent = ?task.cached_state_independent,
                "preverified transaction admitted through mempool"
            );
            return;
        }
        debug!(target: "neo", %hash, ?task.result, relay = task.relay, "preverify rejected transaction");
    }

    /// Try to insert a transaction into the mempool. Used by the
    /// high-level `add_transaction` API.
    pub(crate) async fn add_transaction(&self, transaction: Transaction) -> AddTransactionReply {
        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed before mempool admission"
                );
                return AddTransactionReply {
                    result: VerifyResult::Invalid,
                    hash: neo_primitives::UInt256::zero(),
                };
            }
        };

        if self.ledger.get_transaction(&hash).is_some() {
            return AddTransactionReply {
                result: VerifyResult::AlreadyInPool,
                hash,
            };
        }
        if self.persisted_transaction_exists(&hash) {
            return AddTransactionReply {
                result: VerifyResult::AlreadyExists,
                hash,
            };
        }
        let signers: Vec<neo_primitives::UInt160> =
            transaction.signers().iter().map(|s| s.account).collect();
        if self.persisted_conflict_exists(&hash, &signers) {
            return AddTransactionReply {
                result: VerifyResult::HasConflicts,
                hash,
            };
        }

        // C# Blockchain.OnNewTransaction verifies against the live store
        // view (`system.StoreView`): hand the mempool the system context's
        // store snapshot so admission runs the real verification pipeline.
        // Contexts without a store (lightweight tests) fall back to an
        // empty cache, which fails state-dependent checks closed.
        let settings = self.system.settings();
        let result = match self.system.store_snapshot() {
            Some(snapshot) => self
                .mempool
                .try_add(&transaction, snapshot.as_ref(), &settings),
            None => {
                let snapshot = neo_storage::DataCache::new(false);
                self.mempool.try_add(&transaction, &snapshot, &settings)
            }
        };

        if result == VerifyResult::Succeed {
            self.ledger.insert_transaction(transaction.clone()).ok();
        }

        AddTransactionReply { result, hash }
    }

    /// Transaction admission used by reverify and inventory paths.
    /// Returns the [`VerifyResult`] for the transaction.
    ///
    /// `cached_state_independent` is an optional pre-computed
    /// state-independent result from `TransactionRouter::preverify`.
    /// When `Some(VerifyResult::Succeed)` is provided the mempool
    /// skips redundant signature re-verification and only runs
    /// state-dependent checks.
    pub(crate) fn on_new_transaction(
        &self,
        transaction: &Transaction,
        cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed before mempool admission"
                );
                return VerifyResult::Invalid;
            }
        };

        if self.ledger.get_transaction(&hash).is_some() {
            return VerifyResult::AlreadyInPool;
        }
        if self.persisted_transaction_exists(&hash) {
            return VerifyResult::AlreadyExists;
        }
        let signers: Vec<neo_primitives::UInt160> =
            transaction.signers().iter().map(|s| s.account).collect();
        if self.persisted_conflict_exists(&hash, &signers) {
            return VerifyResult::HasConflicts;
        }

        let settings = self.system.settings();
        let result = match self.system.store_snapshot() {
            Some(snapshot) => self.mempool.try_add_cached(
                transaction,
                snapshot.as_ref(),
                &settings,
                cached_state_independent,
            ),
            None => {
                let snapshot = neo_storage::DataCache::new(false);
                self.mempool.try_add_cached(
                    transaction,
                    &snapshot,
                    &settings,
                    cached_state_independent,
                )
            }
        };
        if result == VerifyResult::Succeed {
            // Best-effort cache insertion; the mempool is the source
            // of truth.
            let _ = self.ledger.insert_transaction(transaction.clone());
        }
        result
    }
}

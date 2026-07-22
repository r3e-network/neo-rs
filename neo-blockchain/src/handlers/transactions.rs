use neo_mempool::TransactionOrigin;
use neo_payloads::Transaction;
use neo_primitives::verify_result::VerifyResult;
use std::time::Instant;
use tracing::debug;

use crate::command::AddTransactionReply;
use crate::ledger_provider::TransactionAdmissionLedger;
use crate::service::{BlockchainService, MempoolLike};

/// C# `Blockchain.MaxTxToReverifyPerIdle`.
const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
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

    /// Try to insert a transaction into the mempool. Used by the
    /// high-level `add_transaction` API.
    pub(crate) async fn add_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: Transaction,
    ) -> AddTransactionReply {
        let started = Instant::now();
        let Some(snapshot) = self.system.store_snapshot() else {
            return AddTransactionReply {
                result: VerifyResult::UnableToVerify,
                hash: transaction.try_hash().unwrap_or_default(),
            };
        };
        let provider =
            TransactionAdmissionLedger::new(self.system.ledger_provider(snapshot.as_ref()));
        let outcome =
            self.mempool
                .add_transaction(origin, &transaction, snapshot.as_ref(), &provider);
        debug!(
            target: "neo::performance",
            elapsed_us = started.elapsed().as_micros() as u64,
            accepted = outcome.is_accepted(),
            "transaction admission completed"
        );
        AddTransactionReply {
            result: outcome.verify_result(),
            hash: outcome.hash().unwrap_or_default(),
        }
    }
}

//! Blockchain handle mempool request methods.
//!
//! This module keeps transaction admission as a typed request/reply facade over
//! `BlockchainCommand::AddTransaction`, while the service loop remains the only
//! place that touches the mempool and live ledger snapshot.

use neo_mempool::TransactionOrigin;
use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::{AddTransactionReply, BlockchainCommand};

impl BlockchainHandle {
    /// Add a transaction to the mempool.
    pub async fn add_transaction(
        &self,
        origin: TransactionOrigin,
        transaction: neo_payloads::Transaction,
    ) -> Result<AddTransactionReply, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::AddTransaction {
                transaction,
                origin,
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }
}

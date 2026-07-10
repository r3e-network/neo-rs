//! Blockchain handle lifecycle commands.
//!
//! Lifecycle commands remain typed methods on [`BlockchainHandle`] so callers
//! do not construct [`BlockchainCommand`] directly. Initialization is a
//! request/reply durability fence; shutdown is a one-way control message.

use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Request blockchain service initialization.
    pub async fn initialize(&self) -> Result<(), ServiceError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::Initialize { reply })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        response
            .await
            .map_err(|_| ServiceError::unavailable("blockchain initialization reply dropped"))?
            .map_err(ServiceError::internal)
    }

    /// Request graceful shutdown of the service loop.
    ///
    /// Shutdown is delivered as a typed command so the service can finish
    /// commands queued before it, then exit even if cloned handles still exist.
    pub async fn shutdown(&self) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::Shutdown)
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }
}

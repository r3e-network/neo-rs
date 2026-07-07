//! Blockchain handle lifecycle commands.
//!
//! Lifecycle commands are one-way control messages for the service actor. They
//! remain typed methods on [`BlockchainHandle`] so callers do not construct
//! [`BlockchainCommand`] directly.

use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Request blockchain service initialization.
    pub async fn initialize(&self) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::Initialize)
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
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

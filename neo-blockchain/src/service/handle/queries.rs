//! Blockchain handle read-query methods.
//!
//! These methods expose request/response reads as ordinary `async fn`s while
//! keeping command construction inside `neo-blockchain`. The service loop still
//! owns the authoritative hot/cold lookup policy.

use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Fetch a block by hash.
    pub async fn get_block(&self, hash: &UInt256) -> Result<Option<Block>, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlock {
                hash: *hash,
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

    /// Fetch a block by canonical height.
    pub async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlockByHeight {
                height,
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

    /// Current canonical tip height.
    pub async fn get_height(&self) -> Result<u32, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetHeight { reply: reply_tx })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }
}

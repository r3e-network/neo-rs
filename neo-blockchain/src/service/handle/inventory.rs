//! Blockchain handle live-inventory submission methods.
//!
//! These one-way methods preserve the live peer/consensus path's service-loop
//! semantics: relay policy, future-block parking, unverified draining, deferred
//! store commits, and mempool maintenance remain inside `BlockchainService`.

use std::sync::Arc;

use neo_payloads::{Block, ExtensiblePayload};
use neo_runtime::ServiceError;

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Submit a peer-relayed inventory block burst to the live sync path.
    ///
    /// This keeps callers on a typed API while preserving the blockchain
    /// service's inventory-specific semantics.
    pub async fn submit_inventory_blocks(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        pre_verified: bool,
    ) -> Result<(), ServiceError> {
        if blocks.is_empty() {
            return Ok(());
        }
        self.cmd_tx
            .send(BlockchainCommand::InventoryBlocks {
                blocks,
                relay,
                pre_verified,
            })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Submit one block to the peer/consensus inventory path.
    ///
    /// Use this for live inventory semantics. RPC and local package imports
    /// should use [`Self::import_block`] or [`Self::import_blocks_bulk`]
    /// instead.
    pub async fn submit_inventory_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::InventoryBlock {
                block,
                relay,
                pre_verified,
            })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Submit an extensible payload to the live inventory path.
    pub async fn submit_inventory_extensible(
        &self,
        payload: ExtensiblePayload,
        relay: bool,
    ) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::InventoryExtensible { payload, relay })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }
}

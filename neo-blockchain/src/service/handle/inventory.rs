//! Blockchain handle live-inventory submission methods.
//!
//! These one-way methods preserve the live peer and local-consensus paths'
//! service-loop semantics: relay policy, future-block parking, unverified
//! draining, deferred store commits, and mempool maintenance remain inside
//! `BlockchainService`.

use std::sync::Arc;

use neo_payloads::{Block, ExtensiblePayload};
use neo_runtime::{CheckedBlockBatch, ServiceError};

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Submit a preflight-checked peer block burst to the live inventory path.
    ///
    /// The checker marker prevents a batch accepted by an arbitrary verifier
    /// from skipping this service's stateless integrity checks. Consensus
    /// witness verification remains mandatory after submission; the token
    /// proves only [`neo_runtime::BlockImport::check`] completed successfully.
    pub async fn submit_checked_inventory_blocks(
        &self,
        checked: CheckedBlockBatch<Arc<Block>, BlockchainHandle>,
        relay: bool,
    ) -> Result<(), ServiceError> {
        if checked.is_empty() {
            return Ok(());
        }
        self.cmd_tx
            .send(BlockchainCommand::CheckedInventoryBlocks { checked, relay })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Submit a block committed by the local consensus engine.
    ///
    /// This is the only public path that skips dBFT witness verification inside
    /// the blockchain service. Peer blocks must use
    /// [`Self::submit_checked_inventory_blocks`]; RPC and local packages use
    /// [`Self::import_block`] or [`Self::import_blocks_bulk`].
    pub async fn submit_consensus_block(
        &self,
        block: Arc<Block>,
        relay: bool,
    ) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::ConsensusBlock { block, relay })
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

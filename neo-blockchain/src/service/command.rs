//! Commands accepted by the [`crate::service::BlockchainService`].
//!
//! The blockchain service is *command-shaped*: every interaction (a
//! consensus-driver block proposal, a network inventory message, an RPC
//! submit, a reverify tick, …) is sent through the same `mpsc::Sender`
//! as a `BlockchainCommand`. The `run()` loop in [`crate::service`]
//! dispatches each command to an `async fn` handler on the service
//! struct, so the dispatch is a single typed `match` against the enum.
//!
//! This is the canonical command set the blockchain service loop drives.
//! The shared cross-crate event type ([`neo_runtime::BlockchainEvent`]) and
//! the default channel capacities live in [`neo_runtime`]; the command enum
//! and its handle are owned here.

use std::sync::Arc;

use neo_payloads::{Block, Transaction, extensible_payload::ExtensiblePayload, header::Header};

use crate::PreverifyCompleted;
use crate::fill_memory_pool::FillMemoryPool;
use crate::import::Import;
use crate::persist_completed::PersistCompleted;
use crate::relay_result::RelayResult;
use crate::reverify::Reverify;

/// Reply payload for [`BlockchainCommand::AddTransaction`].
#[derive(Debug, Clone, Copy)]
pub struct AddTransactionReply {
    /// Verify result of the transaction.
    pub result: neo_primitives::verify_result::VerifyResult,
    /// Hash of the transaction.
    pub hash: neo_primitives::UInt256,
}

/// Reply payload for [`BlockchainCommand::GetHeight`].
pub type HeightReply = u32;

/// Reply payload for [`BlockchainCommand::GetBlock`] /
/// [`BlockchainCommand::GetBlockByHeight`].
pub type BlockReply = Option<Block>;

/// Reply payload for [`BlockchainCommand::ImportBlocks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBlocksReply {
    /// Number of input blocks accepted or already processed before completion
    /// or first failure.
    pub imported: usize,
    /// Late finalization error after the accepted prefix was processed.
    pub error: Option<String>,
}

impl ImportBlocksReply {
    /// Successful import reply.
    #[must_use]
    pub fn ok(imported: usize) -> Self {
        Self {
            imported,
            error: None,
        }
    }

    /// Failed import reply that preserves the accepted prefix count.
    #[must_use]
    pub fn failed(imported: usize, error: impl Into<String>) -> Self {
        Self {
            imported,
            error: Some(error.into()),
        }
    }
}

/// Commands accepted by the blockchain service.
///
/// Note: variants that carry a `oneshot::Sender` for a reply cannot
/// derive `Clone` (a `Sender` is a one-shot channel), so the enum
/// itself is intentionally not `Clone`. Callers that need to keep a
/// copy of a variant should not — the whole point of the command
/// channel is that each command is processed exactly once.
#[derive(Debug)]
pub enum BlockchainCommand {
    /// Notification that a block was persisted.
    PersistCompleted(PersistCompleted),
    /// Request to import blocks.
    Import(Import),
    /// Request to import blocks and report how many directly advanced the tip.
    ImportBlocks {
        /// Blocks to import.
        import: Import,
        /// Reply channel; value is the number of blocks processed from the
        /// supplied batch before completion or first rejected/gapped block,
        /// plus any late finalization error.
        reply: tokio::sync::oneshot::Sender<ImportBlocksReply>,
    },
    /// Request to fill the memory pool.
    FillMemoryPool(FillMemoryPool),
    /// Notification that fill completed.
    FillCompleted,
    /// Request to reverify inventories.
    Reverify(Reverify),
    /// Inventory block received.
    InventoryBlock {
        /// The block.
        block: Arc<Block>,
        /// Whether to relay.
        relay: bool,
        /// Whether state-independent verification (signatures) was already performed.
        pre_verified: bool,
    },
    /// Inventory blocks received as one peer/network burst.
    InventoryBlocks {
        /// Blocks.
        blocks: Vec<Arc<Block>>,
        /// Whether to relay.
        relay: bool,
        /// Whether state-independent verification (signatures) was already performed.
        pre_verified: bool,
    },
    /// Request/response import path for externally supplied blocks.
    ImportBlock {
        /// The block to verify and import.
        block: Arc<Block>,
        /// Reply channel; `true` means the canonical tip advanced.
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    /// Extensible payload received.
    InventoryExtensible {
        /// The extensible payload.
        payload: ExtensiblePayload,
        /// Whether to relay.
        relay: bool,
    },
    /// Preverification completed.
    PreverifyCompleted(PreverifyCompleted),
    /// Headers received.
    Headers(Vec<Header>),
    /// Idle tick for background processing.
    Idle,
    /// Relay result notification.
    RelayResult(RelayResult),
    /// Initialize the blockchain service.
    Initialize,
    /// Check unverified cache and persist any ready consecutive blocks.
    /// Also invoked by the service after a block/import advances the tip so
    /// parked out-of-order blocks continue immediately once their gap closes.
    DrainUnverified,
    /// Attach a new transaction (used by the high-level service API).
    AddTransaction {
        /// The transaction to add.
        transaction: Transaction,
        /// Reply channel.
        reply: tokio::sync::oneshot::Sender<AddTransactionReply>,
    },
    /// Get the current canonical tip height.
    GetHeight {
        /// Reply channel.
        reply: tokio::sync::oneshot::Sender<HeightReply>,
    },
    /// Get a block by its hash.
    GetBlock {
        /// The block hash.
        hash: neo_primitives::UInt256,
        /// Reply channel.
        reply: tokio::sync::oneshot::Sender<BlockReply>,
    },
    /// Get a block by its height.
    GetBlockByHeight {
        /// The block height.
        height: u32,
        /// Reply channel.
        reply: tokio::sync::oneshot::Sender<BlockReply>,
    },
}

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
use std::time::Duration;

use neo_payloads::{Block, Transaction, extensible_payload::ExtensiblePayload, header::Header};
use neo_runtime::CheckedBlockBatch;

use crate::PreverifyCompleted;
use crate::fill_memory_pool::FillMemoryPool;
use crate::handle::BlockchainHandle;
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
    /// Number of input blocks durably accepted or already present before
    /// completion or first failure. An atomic bulk batch that is rewound after
    /// finalization failure contributes zero staged blocks to this count.
    pub imported: usize,
    /// Service-side timing and composition for accepted blocks.
    pub stats: ImportBlocksStats,
    /// Persistence, validation, or late-finalization error.
    pub error: Option<String>,
}

/// Service-side import composition and timing for a processed block batch.
///
/// Callers use this to separate real transaction-bearing work from empty-block
/// fast-forward work without forcing extra command boundaries only for metrics.
/// On an atomic bulk-finalization failure these counters describe staged work,
/// while [`ImportBlocksReply::imported`] remains zero because none became
/// durable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportBlocksStats {
    /// Empty blocks processed through the service's empty-block fast path.
    pub empty_blocks: usize,
    /// Elapsed time spent in empty-block fast paths.
    pub empty_elapsed: Duration,
    /// Transaction-bearing blocks processed through normal persistence.
    pub transaction_blocks: usize,
    /// Elapsed time spent in native/state persistence for transaction-bearing blocks.
    pub transaction_elapsed: Duration,
    /// Elapsed time spent cloning transaction-bearing block payloads for service ownership.
    pub transaction_block_clone_elapsed: Duration,
    /// Elapsed time spent inserting transaction-bearing blocks into the hot ledger cache.
    pub transaction_ledger_insert_elapsed: Duration,
    /// Elapsed time spent running committed hooks for transaction-bearing blocks.
    pub transaction_committed_hook_elapsed: Duration,
    /// Elapsed time spent flushing deferred handlers and the durable store.
    pub finalization_elapsed: Duration,
    /// Elapsed time spent waiting for deferred handlers, including StateService workers.
    pub finalization_commit_handlers_elapsed: Duration,
    /// Elapsed time spent flushing the shared snapshot to the durable store.
    pub finalization_store_commit_elapsed: Duration,
}

impl ImportBlocksStats {
    /// Returns `true` when the reply contains service-side composition data.
    #[must_use]
    pub const fn has_composition(self) -> bool {
        self.empty_blocks > 0 || self.transaction_blocks > 0
    }
}

/// Reply payload for [`BlockchainCommand::ValidateHeaders`].
#[derive(Debug, Clone)]
pub struct HeaderValidationOutcome {
    /// Number of input headers accepted into the verified prefix.
    pub accepted: usize,
    /// The resulting verified frontier after processing the input batch.
    ///
    /// This is the latest accepted header when the batch advanced the frontier,
    /// otherwise the existing cache/store anchor when one was available.
    pub frontier: Option<Header>,
}

impl HeaderValidationOutcome {
    /// Construct a header-validation outcome.
    #[must_use]
    pub const fn new(accepted: usize, frontier: Option<Header>) -> Self {
        Self { accepted, frontier }
    }
}

impl ImportBlocksReply {
    /// Successful import reply.
    #[must_use]
    pub fn ok(imported: usize) -> Self {
        Self {
            imported,
            stats: ImportBlocksStats::default(),
            error: None,
        }
    }

    /// Successful import reply with service-side import statistics.
    #[must_use]
    pub fn ok_with_stats(imported: usize, stats: ImportBlocksStats) -> Self {
        Self {
            imported,
            stats,
            error: None,
        }
    }

    /// Failed import reply with the caller-supplied durable prefix count.
    #[must_use]
    pub fn failed(imported: usize, error: impl Into<String>) -> Self {
        Self {
            imported,
            stats: ImportBlocksStats::default(),
            error: Some(error.into()),
        }
    }

    /// Failed import reply with service-side import statistics.
    #[must_use]
    pub fn failed_with_stats(
        imported: usize,
        stats: ImportBlocksStats,
        error: impl Into<String>,
    ) -> Self {
        Self {
            imported,
            stats,
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
        /// Reply channel; value is the number of blocks durably accepted or
        /// already present before completion or the first failure, plus the
        /// failure when one occurred.
        reply: tokio::sync::oneshot::Sender<ImportBlocksReply>,
    },
    /// Request to fill the memory pool.
    FillMemoryPool(FillMemoryPool),
    /// Notification that fill completed.
    FillCompleted,
    /// Request to reverify inventories.
    Reverify(Reverify),
    /// Block produced and authenticated by the local consensus engine.
    ConsensusBlock {
        /// Locally committed block.
        block: Arc<Block>,
        /// Whether to relay.
        relay: bool,
    },
    /// Preflight-checked inventory blocks received as one peer/network burst.
    CheckedInventoryBlocks {
        /// Checked candidates, including ordered rejection diagnostics. The
        /// checker marker proves the accepted candidates passed this service's
        /// concrete [`BlockchainHandle`] preflight implementation.
        checked: CheckedBlockBatch<Arc<Block>, BlockchainHandle>,
        /// Whether to relay.
        relay: bool,
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
    /// Validate a header batch and report the accepted prefix/frontier.
    ValidateHeaders {
        /// Headers to validate and cache.
        headers: Vec<Header>,
        /// Reply channel for the validated prefix/frontier outcome.
        reply: tokio::sync::oneshot::Sender<HeaderValidationOutcome>,
    },
    /// Idle tick for background processing.
    Idle,
    /// Relay result notification.
    RelayResult(RelayResult),
    /// Initialize the blockchain service and report genesis persistence.
    Initialize {
        /// Initialization result returned after the durable genesis fence.
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    /// Stop the blockchain service command loop after previously queued work.
    Shutdown,
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

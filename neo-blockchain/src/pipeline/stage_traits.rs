//! Pipeline stage traits for block processing.
//!
//! These traits were previously in the `neo-engine` crate (ADR-009/010/026).
//! The `neo-engine` crate was deleted in ADR-027 because its entire public
//! state API (`Pipeline`, `CanonicalChain`, `ChainTip`, `BlockBuffer`) had
//! zero production consumers and `BlockchainEngineAdapter` was never
//! instantiated. The stage traits actually used by the concrete pipeline
//! stages (`ValidateStage`, `ConsensusWitnessStage`, and `PipelineStage`) and
//! their supporting types now live here, next to the implementations that use
//! them.

use std::fmt;

use neo_error::CoreError;
use neo_payloads::Block;
use neo_runtime::BlockOrigin;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the block processing pipeline.
#[derive(Debug, Error)]
pub enum EngineError {
    /// A block failed validation.
    #[error("block validation failed at height {height}: {reason}")]
    ValidationFailed {
        /// The height of the rejected block.
        height: u32,
        /// Human-readable reason for rejection.
        reason: String,
    },

    /// A block failed execution.
    #[error("block execution failed at height {height}: {reason}")]
    ExecutionFailed {
        /// The height of the failed block.
        height: u32,
        /// Human-readable reason for failure.
        reason: String,
    },

    /// A block was received out of order (unexpected height).
    #[error("unexpected block height: expected {expected}, got {actual}")]
    UnexpectedHeight {
        /// The expected next block height.
        expected: u32,
        /// The actual height of the received block.
        actual: u32,
    },

    /// A downstream service error propagated through the pipeline.
    #[error("service error: {0}")]
    Service(#[from] neo_runtime::ServiceError),

    /// A core error propagated from lower layers.
    #[error("{0}")]
    Core(#[from] CoreError),

    /// A pipeline configuration error.
    #[error("pipeline configuration error: {0}")]
    Configuration(String),
}

impl EngineError {
    /// Create a validation error for a block at the given height.
    pub fn validation_failed(height: u32, reason: impl Into<String>) -> Self {
        Self::ValidationFailed {
            height,
            reason: reason.into(),
        }
    }

    /// Create an execution error for a block at the given height.
    pub fn execution_failed(height: u32, reason: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            height,
            reason: reason.into(),
        }
    }

    /// Create an unexpected-height error.
    pub fn unexpected_height(expected: u32, actual: u32) -> Self {
        Self::UnexpectedHeight { expected, actual }
    }

    /// Create a configuration error.
    pub fn configuration(reason: impl Into<String>) -> Self {
        Self::Configuration(reason.into())
    }
}

/// Result type for engine pipeline operations.
pub type EngineResult<T> = std::result::Result<T, EngineError>;

// ---------------------------------------------------------------------------
// Stage vocabulary
// ---------------------------------------------------------------------------

/// Unique identifier for a pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageId {
    /// Block validation (size, timestamp, merkle root, witnesses).
    Validate,
    /// Consensus witness verification against the previous block's committee.
    ConsensusWitness,
    /// Block execution (OnPersist → Application → PostPersist).
    Execute,
    /// Persistence (writing block + state to storage).
    Persist,
    /// Commit (flushing snapshots, updating indexes, notifying plugins).
    Commit,
    /// Post-commit indexing (application logs, token trackers, state root).
    Index,
}

impl StageId {
    /// Human-readable name of this stage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validate => "validate",
            Self::ConsensusWitness => "consensus-witness",
            Self::Execute => "execute",
            Self::Persist => "persist",
            Self::Commit => "commit",
            Self::Index => "index",
        }
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Context passed to each stage during execution.
///
/// This struct carries read-only information about the current pipeline
/// state that stages may need to make decisions.
#[derive(Debug, Clone)]
pub struct StageContext {
    /// The origin of the block being processed.
    pub origin: BlockOrigin,
    /// The current canonical chain tip height (before this block).
    pub current_height: u32,
    /// Whether the pipeline is in bulk-sync (catch-up) mode.
    pub bulk_sync: bool,
}

impl StageContext {
    /// Builds the stage context used by verification-enabled import commands.
    ///
    /// Bulk-sync imports are trusted local replay artifacts; non-bulk imports
    /// enter through RPC-like local submission.
    #[must_use]
    pub fn for_verified_import(current_height: u32, bulk_sync: bool) -> Self {
        Self {
            origin: if bulk_sync {
                BlockOrigin::TrustedLocal
            } else {
                BlockOrigin::Rpc
            },
            current_height,
            bulk_sync,
        }
    }
}

/// Output produced by a stage after processing a block.
#[derive(Debug, Clone)]
pub struct StageOutput {
    /// Duration of this stage's execution in microseconds.
    pub duration_us: u64,
    /// Whether this stage performed any meaningful work (vs. a no-op).
    pub performed_work: bool,
    /// Optional human-readable note for diagnostics.
    pub note: Option<String>,
}

impl StageOutput {
    /// Create a stage output indicating work was performed.
    pub fn performed(duration_us: u64) -> Self {
        Self {
            duration_us,
            performed_work: true,
            note: None,
        }
    }

    /// Create a stage output indicating the stage was a no-op.
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            duration_us: 0,
            performed_work: false,
            note: Some(reason.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Stage traits
// ---------------------------------------------------------------------------

/// A single stage in the block processing pipeline.
///
/// Each stage receives a block and a context, and produces an output
/// describing what it did. Stages should be idempotent where possible —
/// re-running a stage on an already-processed block should be a no-op.
pub trait PipelineStage: Send + Sync + std::fmt::Debug + 'static {
    /// The stage identifier.
    fn id(&self) -> StageId;

    /// Execute this stage for the given block.
    fn execute(&self, ctx: &StageContext, block: &Block) -> EngineResult<StageOutput>;
}

/// Validate stage: checks block structure, timestamps, merkle root,
/// witness scripts, and consensus rules before execution.
pub trait ValidateStage: PipelineStage {
    /// Validate the block. Returns `Ok` if the block passes all checks.
    fn validate(&self, ctx: &StageContext, block: &Block) -> EngineResult<()>;
}

/// Consensus witness stage: verifies that a block header is authorized by the
/// previous block's `NextConsensus` account.
pub trait ConsensusWitnessStage: PipelineStage {
    /// Verify the block header's consensus witness.
    fn verify_consensus_witness(&self, ctx: &StageContext, block: &Block) -> EngineResult<()>;
}

//! # neo-runtime::service::sync_pipeline
//!
//! Shared staged-sync policy and checkpoint primitives.
//!
//! ## Boundary
//!
//! This module belongs to `neo-runtime`. It defines reusable sync-stage
//! contracts and commit-policy decisions, but it does not download blocks,
//! execute NeoVM scripts, write storage, or choose a fork.
//!
//! ## Contents
//!
//! - `SyncStageKind`: stable identifiers for the stages used by sync and
//!   import pipelines.
//! - `CommitPolicy`: Reth-style thresholds that decide when a stage should
//!   durably commit progress.
//! - `SyncStageCheckpointStore`: provider-neutral checkpoint persistence seam
//!   for crash-resumable stages.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use neo_payloads::Block;

use crate::{BlockBatchImportOutcome, BlockOrigin, ImportQueue, ServiceError, ServiceResult};

/// Stable sync-stage identifiers.
///
/// The variants intentionally describe coarse pipeline responsibilities rather
/// than concrete crate names, so callers can keep their stage checkpoints stable
/// while internals move between `neo-network`, `neo-blockchain`, and
/// `neo-state-service`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncStageKind {
    /// Header download and verification.
    Headers,
    /// Body or full-block download.
    Bodies,
    /// Stateless preverification before ordered import.
    Preverify,
    /// Ordered canonical block import.
    Import,
    /// Transaction execution and native persistence.
    Execute,
    /// State-root/MPT projection.
    StateRoot,
    /// Ledger/RPC secondary indexes.
    Index,
    /// Pruning or hot/cold movement after consumer acknowledgements.
    Prune,
}

impl SyncStageKind {
    /// Stable lowercase stage name used for metrics labels and checkpoint keys.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headers => "headers",
            Self::Bodies => "bodies",
            Self::Preverify => "preverify",
            Self::Import => "import",
            Self::Execute => "execute",
            Self::StateRoot => "state_root",
            Self::Index => "index",
            Self::Prune => "prune",
        }
    }
}

/// Durable progress marker for one sync stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncStageCheckpoint {
    /// Stage this checkpoint belongs to.
    pub stage: SyncStageKind,
    /// Highest canonical block height durably completed by the stage.
    pub height: u32,
    /// Number of blocks processed since the stage started or was reset.
    pub processed_blocks: u64,
    /// Approximate number of changed bytes flushed by this checkpoint.
    pub changed_bytes: u64,
}

impl SyncStageCheckpoint {
    /// Construct a checkpoint at `height` for `stage`.
    #[must_use]
    pub const fn new(stage: SyncStageKind, height: u32) -> Self {
        Self {
            stage,
            height,
            processed_blocks: 0,
            changed_bytes: 0,
        }
    }

    /// Attach aggregate processed-block and changed-byte counters.
    #[must_use]
    pub const fn with_counters(mut self, processed_blocks: u64, changed_bytes: u64) -> Self {
        self.processed_blocks = processed_blocks;
        self.changed_bytes = changed_bytes;
        self
    }
}

/// Current in-memory progress for a stage since its last durable checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StageProgress {
    /// Blocks processed since the last commit.
    pub blocks: u64,
    /// Changed key/value operations or bytes, depending on the stage.
    pub changes: u64,
    /// Cumulative GAS executed by the stage window.
    pub cumulative_gas: u64,
    /// Wall-clock time since the stage window began.
    pub elapsed: Duration,
}

impl StageProgress {
    /// Construct progress from block count only.
    #[must_use]
    pub const fn blocks(blocks: u64) -> Self {
        Self {
            blocks,
            changes: 0,
            cumulative_gas: 0,
            elapsed: Duration::ZERO,
        }
    }
}

/// One contiguous block batch entering a staged sync/import driver.
#[derive(Clone, Debug)]
pub struct SyncBlockBatch {
    /// Height of the first block in `blocks`.
    pub start_height: u32,
    /// Blocks in canonical order.
    pub blocks: Vec<Block>,
    /// Approximate bytes changed by this stage batch, when known.
    ///
    /// Download-only callers can leave this at zero. Stages that already know
    /// their write-set size should fill it so `CommitPolicy::max_changes`
    /// can make an IO-aware decision.
    pub changed_bytes: u64,
}

impl SyncBlockBatch {
    /// Construct a sync batch.
    #[must_use]
    pub fn new(start_height: u32, blocks: Vec<Block>) -> Self {
        Self {
            start_height,
            blocks,
            changed_bytes: 0,
        }
    }

    /// Attach the approximate changed-byte count for commit-policy decisions.
    #[must_use]
    pub const fn with_changed_bytes(mut self, changed_bytes: u64) -> Self {
        self.changed_bytes = changed_bytes;
        self
    }

    /// Height immediately after the last block in this batch.
    #[must_use]
    pub fn next_height(&self) -> u32 {
        self.start_height
            .saturating_add(u32::try_from(self.blocks.len()).unwrap_or(u32::MAX))
    }

    /// Returns `true` when this batch carries no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Result of pushing one sync batch through a [`SyncPipelineDriver`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncPipelineImportOutcome {
    /// Outcome returned by the ordered import queue.
    pub imported: BlockBatchImportOutcome,
    /// Height expected for the next non-empty batch.
    pub next_height: Option<u32>,
    /// Checkpoint persisted after this batch, when the commit policy fired.
    pub checkpoint: Option<SyncStageCheckpoint>,
}

/// Runtime sync driver that bridges downloaded batches to the import queue.
///
/// The driver enforces contiguous heights, submits each batch through the
/// shared [`ImportQueue`], and writes an `Import` stage checkpoint when the
/// configured [`CommitPolicy`] fires. It intentionally does not know where
/// blocks came from: P2P, fast-sync packages, and future state-sync adapters
/// can all map their input into [`SyncBlockBatch`].
pub struct SyncPipelineDriver<Q: ImportQueue + ?Sized, C: SyncStageCheckpointStore + ?Sized> {
    import_queue: Arc<Q>,
    checkpoints: Arc<C>,
    commit_policy: CommitPolicy,
    origin: BlockOrigin,
    progress: StageProgress,
    window_started: Instant,
    next_height: Option<u32>,
    total_blocks: u64,
    total_changes: u64,
}

impl<Q, C> SyncPipelineDriver<Q, C>
where
    Q: ImportQueue + ?Sized,
    C: SyncStageCheckpointStore + ?Sized,
{
    /// Create a driver from the last durable import checkpoint.
    pub fn new(
        import_queue: Arc<Q>,
        checkpoints: Arc<C>,
        commit_policy: CommitPolicy,
        origin: BlockOrigin,
    ) -> ServiceResult<Self> {
        let checkpoint = checkpoints.checkpoint(SyncStageKind::Import)?;
        let next_height = checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.height.saturating_add(1));
        let total_blocks = checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.processed_blocks);
        let total_changes = checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.changed_bytes);

        Ok(Self {
            import_queue,
            checkpoints,
            commit_policy,
            origin,
            progress: StageProgress::default(),
            window_started: Instant::now(),
            next_height,
            total_blocks,
            total_changes,
        })
    }

    /// Height expected for the next non-empty batch.
    #[must_use]
    pub const fn next_height(&self) -> Option<u32> {
        self.next_height
    }

    /// Import one contiguous batch and checkpoint if the policy fires.
    pub async fn import_batch(
        &mut self,
        batch: SyncBlockBatch,
    ) -> ServiceResult<SyncPipelineImportOutcome> {
        self.validate_batch(&batch)?;
        if batch.is_empty() {
            return Ok(SyncPipelineImportOutcome {
                imported: BlockBatchImportOutcome::new(0),
                next_height: self.next_height,
                checkpoint: None,
            });
        }

        let block_count = batch.blocks.len();
        let block_count_u64 = u64::try_from(block_count)
            .map_err(|_| ServiceError::invalid_input("sync batch contains too many blocks"))?;
        let changed_bytes = batch.changed_bytes;
        let last_height = batch
            .blocks
            .last()
            .map(Block::index)
            .expect("non-empty batch has a last block");
        let next_height = batch.next_height();

        let imported = self
            .import_queue
            .push_blocks(batch.blocks, self.origin)
            .await?;
        if imported.processed != block_count {
            return Err(ServiceError::internal(format!(
                "import queue processed {} blocks from a {} block sync batch",
                imported.processed, block_count
            )));
        }

        self.next_height = Some(next_height);
        self.progress.blocks = self.progress.blocks.saturating_add(block_count_u64);
        self.progress.changes = self.progress.changes.saturating_add(changed_bytes);
        self.progress.elapsed = self.window_started.elapsed();
        self.total_blocks = self.total_blocks.saturating_add(block_count_u64);
        self.total_changes = self.total_changes.saturating_add(changed_bytes);

        let checkpoint = if self.commit_policy.evaluate(self.progress).should_commit {
            let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, last_height)
                .with_counters(self.total_blocks, self.total_changes);
            self.checkpoints.save_checkpoint(checkpoint.clone())?;
            self.progress = StageProgress::default();
            self.window_started = Instant::now();
            Some(checkpoint)
        } else {
            None
        };

        Ok(SyncPipelineImportOutcome {
            imported,
            next_height: self.next_height,
            checkpoint,
        })
    }

    fn validate_batch(&self, batch: &SyncBlockBatch) -> ServiceResult<()> {
        if let Some(expected_height) = self.next_height
            && batch.start_height != expected_height
        {
            return Err(ServiceError::invalid_state(format!(
                "sync pipeline expected block height {expected_height}, got {}",
                batch.start_height
            )));
        }

        for (offset, block) in batch.blocks.iter().enumerate() {
            let offset = u32::try_from(offset)
                .map_err(|_| ServiceError::invalid_input("sync batch offset overflows u32"))?;
            let declared_height = batch.start_height.checked_add(offset).ok_or_else(|| {
                ServiceError::invalid_input("sync batch height arithmetic overflow")
            })?;
            if block.index() != declared_height {
                return Err(ServiceError::invalid_input(format!(
                    "sync batch declares height {declared_height}, but block header has height {}",
                    block.index()
                )));
            }
        }

        Ok(())
    }
}

/// Threshold that caused a commit decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitTrigger {
    /// `max_blocks` was reached.
    Blocks,
    /// `max_changes` was reached.
    Changes,
    /// `max_cumulative_gas` was reached.
    CumulativeGas,
    /// `max_duration` was reached.
    Duration,
}

/// Result of evaluating a [`CommitPolicy`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitDecision {
    /// `true` when the stage should flush and checkpoint now.
    pub should_commit: bool,
    /// Threshold that caused the decision, if any.
    pub trigger: Option<CommitTrigger>,
}

impl CommitDecision {
    /// Commit now because `trigger` fired.
    #[must_use]
    pub const fn commit(trigger: CommitTrigger) -> Self {
        Self {
            should_commit: true,
            trigger: Some(trigger),
        }
    }

    /// Keep accumulating work in the current stage window.
    #[must_use]
    pub const fn continue_stage() -> Self {
        Self {
            should_commit: false,
            trigger: None,
        }
    }
}

/// Reth-style stage commit thresholds.
///
/// The first configured threshold reached by [`CommitPolicy::evaluate`] wins.
/// All thresholds are optional so stages can tune the memory, IO, and latency
/// tradeoff independently.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitPolicy {
    /// Commit after this many blocks.
    pub max_blocks: Option<u64>,
    /// Commit after this many changes.
    pub max_changes: Option<u64>,
    /// Commit after this much cumulative GAS.
    pub max_cumulative_gas: Option<u64>,
    /// Commit after this much wall-clock time.
    pub max_duration: Option<Duration>,
}

impl CommitPolicy {
    /// Policy that commits every block.
    #[must_use]
    pub const fn per_block() -> Self {
        Self {
            max_blocks: Some(1),
            max_changes: None,
            max_cumulative_gas: None,
            max_duration: None,
        }
    }

    /// Set a block threshold. `0` disables the threshold.
    #[must_use]
    pub const fn with_max_blocks(mut self, max_blocks: u64) -> Self {
        self.max_blocks = if max_blocks == 0 {
            None
        } else {
            Some(max_blocks)
        };
        self
    }

    /// Set a change threshold. `0` disables the threshold.
    #[must_use]
    pub const fn with_max_changes(mut self, max_changes: u64) -> Self {
        self.max_changes = if max_changes == 0 {
            None
        } else {
            Some(max_changes)
        };
        self
    }

    /// Set a cumulative-GAS threshold. `0` disables the threshold.
    #[must_use]
    pub const fn with_max_cumulative_gas(mut self, max_cumulative_gas: u64) -> Self {
        self.max_cumulative_gas = if max_cumulative_gas == 0 {
            None
        } else {
            Some(max_cumulative_gas)
        };
        self
    }

    /// Set a duration threshold. `Duration::ZERO` disables the threshold.
    #[must_use]
    pub const fn with_max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = if max_duration.is_zero() {
            None
        } else {
            Some(max_duration)
        };
        self
    }

    /// Evaluate the policy against the current stage progress.
    #[must_use]
    pub fn evaluate(&self, progress: StageProgress) -> CommitDecision {
        if self
            .max_blocks
            .is_some_and(|max_blocks| progress.blocks >= max_blocks)
        {
            return CommitDecision::commit(CommitTrigger::Blocks);
        }
        if self
            .max_changes
            .is_some_and(|max_changes| progress.changes >= max_changes)
        {
            return CommitDecision::commit(CommitTrigger::Changes);
        }
        if self
            .max_cumulative_gas
            .is_some_and(|max_gas| progress.cumulative_gas >= max_gas)
        {
            return CommitDecision::commit(CommitTrigger::CumulativeGas);
        }
        if self
            .max_duration
            .is_some_and(|max_duration| progress.elapsed >= max_duration)
        {
            return CommitDecision::commit(CommitTrigger::Duration);
        }
        CommitDecision::continue_stage()
    }
}

/// Provider-neutral checkpoint store for staged sync.
pub trait SyncStageCheckpointStore: Send + Sync {
    /// Return the last durable checkpoint for `stage`, if present.
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>>;

    /// Persist a new checkpoint for its stage.
    fn save_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()>;
}

/// In-memory checkpoint store for tests and ephemeral sync drivers.
#[derive(Debug, Default)]
pub struct InMemorySyncStageCheckpointStore {
    checkpoints: RwLock<BTreeMap<SyncStageKind, SyncStageCheckpoint>>,
}

impl InMemorySyncStageCheckpointStore {
    /// Construct an empty in-memory checkpoint store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SyncStageCheckpointStore for InMemorySyncStageCheckpointStore {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        let checkpoints = self
            .checkpoints
            .read()
            .map_err(|_| ServiceError::internal("sync checkpoint store lock poisoned"))?;
        Ok(checkpoints.get(&stage).cloned())
    }

    fn save_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        let mut checkpoints = self
            .checkpoints
            .write()
            .map_err(|_| ServiceError::internal("sync checkpoint store lock poisoned"))?;
        checkpoints.insert(checkpoint.stage, checkpoint);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/service/sync_pipeline.rs"]
mod tests;

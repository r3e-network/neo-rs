//! Shared staged-sync policy and checkpoint primitives.
//!
//! This module defines reusable contracts for sync-stage progress, commit
//! policy, and ordered block-batch import. It intentionally does not download
//! blocks, execute NeoVM scripts, write storage, or choose a fork.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neo_payloads::Block;
use neo_storage::persistence::Store;
use parking_lot::RwLock;

use crate::{BlockBatchImportOutcome, BlockOrigin, ImportQueue, ServiceError, ServiceResult};

/// Stable sync-stage identifiers.
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
    #[must_use]
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

    const fn code(self) -> u8 {
        match self {
            Self::Headers => 0,
            Self::Bodies => 1,
            Self::Preverify => 2,
            Self::Import => 3,
            Self::Execute => 4,
            Self::StateRoot => 5,
            Self::Index => 6,
            Self::Prune => 7,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Headers),
            1 => Some(Self::Bodies),
            2 => Some(Self::Preverify),
            3 => Some(Self::Import),
            4 => Some(Self::Execute),
            5 => Some(Self::StateRoot),
            6 => Some(Self::Index),
            7 => Some(Self::Prune),
            _ => None,
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

/// Reth-style commit policy for sync stages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommitPolicy {
    /// Commit after this many blocks, when set.
    pub max_blocks: Option<u64>,
    /// Commit after this many changes/bytes, when set.
    pub max_changes: Option<u64>,
    /// Commit after this much cumulative GAS, when set.
    pub max_cumulative_gas: Option<u64>,
    /// Commit after this much wall-clock time, when set.
    pub max_duration: Option<Duration>,
}

impl CommitPolicy {
    /// Construct an empty policy. It never fires until a threshold is set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_blocks: None,
            max_changes: None,
            max_cumulative_gas: None,
            max_duration: None,
        }
    }

    /// Set the block threshold.
    #[must_use]
    pub const fn with_max_blocks(mut self, max_blocks: u64) -> Self {
        self.max_blocks = Some(max_blocks);
        self
    }

    /// Set the change/byte threshold.
    #[must_use]
    pub const fn with_max_changes(mut self, max_changes: u64) -> Self {
        self.max_changes = Some(max_changes);
        self
    }

    /// Set the cumulative GAS threshold.
    #[must_use]
    pub const fn with_max_cumulative_gas(mut self, max_cumulative_gas: u64) -> Self {
        self.max_cumulative_gas = Some(max_cumulative_gas);
        self
    }

    /// Set the elapsed-time threshold.
    #[must_use]
    pub const fn with_max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = Some(max_duration);
        self
    }

    /// Returns `true` when any configured threshold has fired.
    #[must_use]
    pub fn should_commit(self, progress: StageProgress) -> bool {
        self.max_blocks.is_some_and(|max| progress.blocks >= max)
            || self.max_changes.is_some_and(|max| progress.changes >= max)
            || self
                .max_cumulative_gas
                .is_some_and(|max| progress.cumulative_gas >= max)
            || self.max_duration.is_some_and(|max| progress.elapsed >= max)
    }
}

/// Provider-neutral checkpoint persistence seam for sync stages.
pub trait SyncStageCheckpointStore: Send + Sync {
    /// Read the latest checkpoint for `stage`.
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>>;

    /// Persist a checkpoint for its stage.
    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()>;
}

/// In-memory checkpoint store for tests and composition scaffolding.
#[derive(Debug, Default)]
pub struct InMemorySyncStageCheckpointStore {
    checkpoints: RwLock<BTreeMap<SyncStageKind, SyncStageCheckpoint>>,
}

impl SyncStageCheckpointStore for InMemorySyncStageCheckpointStore {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        Ok(self.checkpoints.read().get(&stage).cloned())
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        self.checkpoints
            .write()
            .insert(checkpoint.stage, checkpoint);
        Ok(())
    }
}

const STORE_CHECKPOINT_KEY_PREFIX: [u8; 2] = [0xF8, b's'];
const STORE_CHECKPOINT_VALUE_MAGIC: &[u8; 6] = b"NRSCP1";
const STORE_CHECKPOINT_VALUE_LEN: usize = STORE_CHECKPOINT_VALUE_MAGIC.len() + 1 + 4 + 8 + 8;

/// Store-backed checkpoint provider for crash-resumable sync stages.
///
/// The key is deliberately three bytes (`0xF8`, `s`, stage-code), shorter than
/// every consensus [`neo_storage::StorageKey`] row, whose serialized form starts
/// with a four-byte contract id. This keeps runtime sync metadata out of the
/// contract-storage keyspace while still using the same provider/factory-backed
/// storage engine. Values are versioned fixed-width records:
/// `NRSCP1 || stage || height_be || processed_blocks_be || changed_bytes_be`.
#[derive(Debug)]
pub struct StoreSyncStageCheckpointStore<S: Store + Clone> {
    store: S,
    write_lock: parking_lot::Mutex<()>,
}

impl<S> StoreSyncStageCheckpointStore<S>
where
    S: Store + Clone,
{
    /// Create a checkpoint store over a concrete storage backend handle.
    ///
    /// Cloned store handles are expected to observe the same backend state,
    /// which is true for the built-in memory, MDBX, and RocksDB stores.
    #[must_use]
    pub fn new(store: S) -> Self {
        Self {
            store,
            write_lock: parking_lot::Mutex::new(()),
        }
    }

    /// Return the underlying store handle.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }
}

impl<S> SyncStageCheckpointStore for StoreSyncStageCheckpointStore<S>
where
    S: Store + Clone,
{
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        let Some(bytes) = self.store.try_get_bytes(&checkpoint_key(stage)) else {
            return Ok(None);
        };
        decode_checkpoint(stage, &bytes).map(Some)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        let key = checkpoint_key(checkpoint.stage);
        let value = encode_checkpoint(&checkpoint);

        if let Some(raw_overlay) = self.store.as_raw_overlay_store() {
            let overlay = [(key.clone(), Some(value.clone()))];
            if raw_overlay
                .try_commit_raw_overlay(&overlay)
                .map_err(|err| storage_error("write sync checkpoint", err))?
            {
                self.store
                    .flush()
                    .map_err(|err| storage_error("flush sync checkpoint", err))?;
                return Ok(());
            }
        }

        let _guard = self.write_lock.lock();
        let mut writer = self.store.clone();
        writer
            .put_sync(key, value)
            .map_err(|err| storage_error("write sync checkpoint", err))?;
        writer
            .flush()
            .map_err(|err| storage_error("flush sync checkpoint", err))
    }
}

fn checkpoint_key(stage: SyncStageKind) -> Vec<u8> {
    vec![
        STORE_CHECKPOINT_KEY_PREFIX[0],
        STORE_CHECKPOINT_KEY_PREFIX[1],
        stage.code(),
    ]
}

fn encode_checkpoint(checkpoint: &SyncStageCheckpoint) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(STORE_CHECKPOINT_VALUE_LEN);
    bytes.extend_from_slice(STORE_CHECKPOINT_VALUE_MAGIC);
    bytes.push(checkpoint.stage.code());
    bytes.extend_from_slice(&checkpoint.height.to_be_bytes());
    bytes.extend_from_slice(&checkpoint.processed_blocks.to_be_bytes());
    bytes.extend_from_slice(&checkpoint.changed_bytes.to_be_bytes());
    bytes
}

fn decode_checkpoint(
    expected_stage: SyncStageKind,
    bytes: &[u8],
) -> ServiceResult<SyncStageCheckpoint> {
    if bytes.len() != STORE_CHECKPOINT_VALUE_LEN
        || &bytes[..STORE_CHECKPOINT_VALUE_MAGIC.len()] != STORE_CHECKPOINT_VALUE_MAGIC
    {
        return Err(ServiceError::invalid_state(format!(
            "invalid sync checkpoint payload for stage {}: {} bytes",
            expected_stage.as_str(),
            bytes.len()
        )));
    }

    let mut cursor = STORE_CHECKPOINT_VALUE_MAGIC.len();
    let stage_code = bytes[cursor];
    cursor += 1;
    let stage = SyncStageKind::from_code(stage_code).ok_or_else(|| {
        ServiceError::invalid_state(format!("invalid sync checkpoint stage code {stage_code}"))
    })?;
    if stage != expected_stage {
        return Err(ServiceError::invalid_state(format!(
            "sync checkpoint stage mismatch: requested {}, stored {}",
            expected_stage.as_str(),
            stage.as_str()
        )));
    }

    let height = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().expect("slice length"));
    cursor += 4;
    let processed_blocks =
        u64::from_be_bytes(bytes[cursor..cursor + 8].try_into().expect("slice length"));
    cursor += 8;
    let changed_bytes =
        u64::from_be_bytes(bytes[cursor..cursor + 8].try_into().expect("slice length"));

    Ok(SyncStageCheckpoint::new(stage, height).with_counters(processed_blocks, changed_bytes))
}

fn storage_error(context: &'static str, err: impl std::fmt::Display) -> ServiceError {
    ServiceError::internal(format!("{context}: {err}"))
}

/// One contiguous block batch entering a staged sync/import driver.
#[derive(Clone, Debug)]
pub struct SyncBlockBatch {
    /// Height of the first block in `blocks`.
    pub start_height: u32,
    /// Blocks in canonical order.
    pub blocks: Vec<Block>,
    /// Approximate bytes changed by this stage batch, when known.
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
    /// Outcome returned by the ordered block import path.
    pub imported: BlockBatchImportOutcome,
    /// Height expected for the next non-empty batch.
    pub next_height: Option<u32>,
    /// Checkpoint persisted after this batch, when the commit policy fired.
    pub checkpoint: Option<SyncStageCheckpoint>,
}

/// Runtime sync driver that bridges downloaded batches to block import.
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
        let next_height = checkpoint.as_ref().map(|checkpoint| {
            checkpoint
                .height
                .checked_add(1)
                .unwrap_or(checkpoint.height)
        });
        Ok(Self {
            import_queue,
            checkpoints,
            commit_policy,
            origin,
            progress: StageProgress::default(),
            window_started: Instant::now(),
            next_height,
            total_blocks: 0,
            total_changes: 0,
        })
    }

    /// Push one contiguous block batch through the canonical import path.
    pub async fn push_batch(
        &mut self,
        batch: SyncBlockBatch,
    ) -> ServiceResult<SyncPipelineImportOutcome> {
        if batch.is_empty() {
            return Ok(SyncPipelineImportOutcome {
                imported: BlockBatchImportOutcome::new(0),
                next_height: self.next_height,
                checkpoint: None,
            });
        }

        if let Some(expected) = self.next_height {
            if batch.start_height != expected {
                return Err(ServiceError::invalid_input(format!(
                    "non-contiguous sync batch: expected height {expected}, got {}",
                    batch.start_height
                )));
            }
        }

        let next_height = batch.next_height();
        let imported = self
            .import_queue
            .push_blocks(batch.blocks, self.origin)
            .await?;
        let processed_blocks = u64::try_from(imported.processed).unwrap_or(u64::MAX);
        self.progress.blocks = self.progress.blocks.saturating_add(processed_blocks);
        self.progress.changes = self.progress.changes.saturating_add(batch.changed_bytes);
        self.progress.elapsed = self.window_started.elapsed();
        self.total_blocks = self.total_blocks.saturating_add(processed_blocks);
        self.total_changes = self.total_changes.saturating_add(batch.changed_bytes);
        self.next_height = Some(next_height);

        let checkpoint = if self.commit_policy.should_commit(self.progress) {
            let checkpoint =
                SyncStageCheckpoint::new(SyncStageKind::Import, next_height.saturating_sub(1))
                    .with_counters(self.total_blocks, self.total_changes);
            self.checkpoints.put_checkpoint(checkpoint.clone())?;
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
}

#[cfg(test)]
#[path = "../tests/service/sync_pipeline.rs"]
mod tests;

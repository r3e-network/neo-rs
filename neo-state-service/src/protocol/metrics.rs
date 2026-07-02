use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of state root ingestion counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootIngestStats {
    /// Number of accepted state roots.
    pub accepted: u64,
    /// Number of rejected state roots.
    pub rejected: u64,
}

/// Snapshot of local StateService MPT apply counters and EWMA timings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootApplyStats {
    /// Number of blocks for which local MPT apply was attempted.
    pub attempts: u64,
    /// Number of failed local MPT apply attempts.
    pub failures: u64,
    /// Latest block height observed by the local MPT apply path.
    pub latest_height: u64,
    /// EWMA total local MPT apply time, in microseconds.
    pub avg_total_us: u64,
    /// EWMA snapshot-to-MPT changeset projection time, in microseconds.
    pub avg_project_us: u64,
    /// EWMA trie/write application time, in microseconds.
    pub avg_apply_us: u64,
    /// EWMA count of projected MPT changes per block.
    pub avg_changes: u64,
}

/// Fine-grained timing stage inside local StateService MPT application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRootApplyStage {
    /// Time the producer spends sending into the bounded async MPT queue.
    EnqueueBlocking,
    /// Time spent queued before the async MPT worker begins applying a block.
    QueueWait,
    /// Apply the projected storage changes to the in-memory trie.
    MutateChanges,
    /// Compute the new root hash after all mutations.
    RootHash,
    /// Commit dirty trie nodes into the MPT write batch.
    TrieCommit,
    /// Add the local state-root record and current-index record to the write batch.
    OverlayPrepare,
    /// Persist the write batch to the optional backing store.
    BackingCommit,
    /// Publish the write batch into the live in-memory generation.
    PublishGeneration,
}

impl StateRootApplyStage {
    fn label(self) -> &'static str {
        match self {
            Self::EnqueueBlocking => "enqueue_blocking",
            Self::QueueWait => "queue_wait",
            Self::MutateChanges => "mutate_changes",
            Self::RootHash => "root_hash",
            Self::TrieCommit => "trie_commit",
            Self::OverlayPrepare => "overlay_prepare",
            Self::BackingCommit => "backing_commit",
            Self::PublishGeneration => "publish_generation",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::EnqueueBlocking => 0,
            Self::QueueWait => 1,
            Self::MutateChanges => 2,
            Self::RootHash => 3,
            Self::TrieCommit => 4,
            Self::OverlayPrepare => 5,
            Self::BackingCommit => 6,
            Self::PublishGeneration => 7,
        }
    }
}

/// Item counts observed inside local StateService MPT application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRootApplyCountKind {
    /// Blocks drained and applied by one async MPT worker batch.
    BatchBlocks,
    /// Projected block storage changes supplied to the trie.
    Changes,
    /// All write-batch entries produced by trie commit plus local root records.
    OverlayEntries,
    /// Write-batch entries that put/update data.
    OverlayPuts,
    /// Write-batch entries that delete data.
    OverlayDeletes,
}

impl StateRootApplyCountKind {
    fn label(self) -> &'static str {
        match self {
            Self::BatchBlocks => "batch_blocks",
            Self::Changes => "changes",
            Self::OverlayEntries => "overlay_entries",
            Self::OverlayPuts => "overlay_puts",
            Self::OverlayDeletes => "overlay_deletes",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::BatchBlocks => 0,
            Self::Changes => 1,
            Self::OverlayEntries => 2,
            Self::OverlayPuts => 3,
            Self::OverlayDeletes => 4,
        }
    }
}

/// Snapshot of one fine-grained StateService MPT apply stage metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootApplyStageStats {
    /// Stage label used in Prometheus output.
    pub stage: &'static str,
    /// Total stage observations recorded since process start.
    pub calls: u64,
    /// EWMA stage duration in microseconds.
    pub avg_us: u64,
}

/// Snapshot of one StateService MPT apply count metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootApplyCountStats {
    /// Count label used in Prometheus output.
    pub kind: &'static str,
    /// Number of apply attempts that have recorded this count.
    pub samples: u64,
    /// Cumulative item count since process start.
    pub total: u64,
    /// EWMA item count per apply attempt.
    pub avg: u64,
}

/// Direct snapshot of the MPT apply fields used by hot import progress logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateRootApplyHotStats {
    /// EWMA producer send duration for the bounded async MPT queue, in microseconds.
    pub enqueue_blocking_avg_us: u64,
    /// EWMA queue-wait duration before the async MPT worker starts a block, in microseconds.
    pub queue_wait_avg_us: u64,
    /// EWMA mutation stage duration in microseconds.
    pub mutate_changes_avg_us: u64,
    /// EWMA root-hash stage duration in microseconds.
    pub root_hash_avg_us: u64,
    /// EWMA trie-commit stage duration in microseconds.
    pub trie_commit_avg_us: u64,
    /// EWMA backing-store commit stage duration in microseconds.
    pub backing_commit_avg_us: u64,
    /// EWMA generation-publish stage duration in microseconds.
    pub publish_generation_avg_us: u64,
    /// EWMA number of overlay entries per apply.
    pub overlay_entries_avg: u64,
    /// EWMA number of blocks drained by one async worker batch.
    pub batch_blocks_avg: u64,
}

#[derive(Debug)]
struct TimingMetricSlot {
    calls: AtomicU64,
    avg_us: AtomicU64,
}

impl TimingMetricSlot {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            avg_us: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct CountMetricSlot {
    samples: AtomicU64,
    total: AtomicU64,
    avg: AtomicU64,
}

impl CountMetricSlot {
    const fn new() -> Self {
        Self {
            samples: AtomicU64::new(0),
            total: AtomicU64::new(0),
            avg: AtomicU64::new(0),
        }
    }
}

static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static APPLY_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static APPLY_FAILURES: AtomicU64 = AtomicU64::new(0);
static APPLY_HEIGHT: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_PROJECT_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_APPLY_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_CHANGES: AtomicU64 = AtomicU64::new(0);
static APPLY_STAGE_ORDER: [StateRootApplyStage; 8] = [
    StateRootApplyStage::EnqueueBlocking,
    StateRootApplyStage::QueueWait,
    StateRootApplyStage::MutateChanges,
    StateRootApplyStage::RootHash,
    StateRootApplyStage::TrieCommit,
    StateRootApplyStage::OverlayPrepare,
    StateRootApplyStage::BackingCommit,
    StateRootApplyStage::PublishGeneration,
];
static APPLY_STAGES: [TimingMetricSlot; 8] = [
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
];
static APPLY_COUNT_ORDER: [StateRootApplyCountKind; 5] = [
    StateRootApplyCountKind::BatchBlocks,
    StateRootApplyCountKind::Changes,
    StateRootApplyCountKind::OverlayEntries,
    StateRootApplyCountKind::OverlayPuts,
    StateRootApplyCountKind::OverlayDeletes,
];
static APPLY_COUNTS: [CountMetricSlot; 5] = [
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
];

/// Namespace for state root ingestion metric helpers.
pub struct StateRootIngestMetrics;

impl StateRootIngestMetrics {
    /// Records the outcome of processing a state root from the network.
    pub fn record_ingest_result(accepted: bool) {
        if accepted {
            ACCEPTED.fetch_add(1, Ordering::Relaxed);
        } else {
            REJECTED.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Returns the current ingestion counters.
    pub fn state_root_ingest_stats() -> StateRootIngestStats {
        StateRootIngestStats {
            accepted: ACCEPTED.load(Ordering::Relaxed),
            rejected: REJECTED.load(Ordering::Relaxed),
        }
    }
}

/// Namespace for local StateService MPT apply metric helpers.
pub struct StateRootApplyMetrics;

impl StateRootApplyMetrics {
    /// Records one local MPT apply attempt from the committing hook.
    pub fn record_apply(
        block_index: u32,
        changes: usize,
        project_us: u64,
        apply_us: u64,
        total_us: u64,
        success: bool,
    ) {
        APPLY_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        if !success {
            APPLY_FAILURES.fetch_add(1, Ordering::Relaxed);
        }
        APPLY_HEIGHT.store(block_index as u64, Ordering::Relaxed);
        ewma(&APPLY_AVG_TOTAL_US, total_us);
        ewma(&APPLY_AVG_PROJECT_US, project_us);
        ewma(&APPLY_AVG_APPLY_US, apply_us);
        ewma(&APPLY_AVG_CHANGES, changes as u64);
    }

    /// Records one fine-grained local MPT apply stage.
    pub fn record_stage(stage: StateRootApplyStage, elapsed_us: u64) {
        let slot = &APPLY_STAGES[stage.slot_index()];
        slot.calls.fetch_add(1, Ordering::Relaxed);
        ewma(&slot.avg_us, elapsed_us);
    }

    /// Records one local MPT apply item count.
    pub fn record_count(kind: StateRootApplyCountKind, count: u64) {
        let slot = &APPLY_COUNTS[kind.slot_index()];
        slot.samples.fetch_add(1, Ordering::Relaxed);
        slot.total.fetch_add(count, Ordering::Relaxed);
        ewma(&slot.avg, count);
    }

    /// Returns the current local MPT apply counters and EWMA timings.
    pub fn state_root_apply_stats() -> StateRootApplyStats {
        StateRootApplyStats {
            attempts: APPLY_ATTEMPTS.load(Ordering::Relaxed),
            failures: APPLY_FAILURES.load(Ordering::Relaxed),
            latest_height: APPLY_HEIGHT.load(Ordering::Relaxed),
            avg_total_us: APPLY_AVG_TOTAL_US.load(Ordering::Relaxed),
            avg_project_us: APPLY_AVG_PROJECT_US.load(Ordering::Relaxed),
            avg_apply_us: APPLY_AVG_APPLY_US.load(Ordering::Relaxed),
            avg_changes: APPLY_AVG_CHANGES.load(Ordering::Relaxed),
        }
    }

    /// Returns the fixed MPT apply metrics used in fast import progress logs
    /// without allocating the full telemetry vectors.
    pub fn state_root_apply_hot_stats() -> StateRootApplyHotStats {
        StateRootApplyHotStats {
            enqueue_blocking_avg_us: stage_avg(StateRootApplyStage::EnqueueBlocking),
            queue_wait_avg_us: stage_avg(StateRootApplyStage::QueueWait),
            mutate_changes_avg_us: stage_avg(StateRootApplyStage::MutateChanges),
            root_hash_avg_us: stage_avg(StateRootApplyStage::RootHash),
            trie_commit_avg_us: stage_avg(StateRootApplyStage::TrieCommit),
            backing_commit_avg_us: stage_avg(StateRootApplyStage::BackingCommit),
            publish_generation_avg_us: stage_avg(StateRootApplyStage::PublishGeneration),
            overlay_entries_avg: count_avg(StateRootApplyCountKind::OverlayEntries),
            batch_blocks_avg: count_avg(StateRootApplyCountKind::BatchBlocks),
        }
    }

    /// Returns fine-grained local MPT apply stage timing metrics.
    pub fn state_root_apply_stage_stats() -> Vec<StateRootApplyStageStats> {
        APPLY_STAGE_ORDER
            .iter()
            .map(|stage| {
                let slot = &APPLY_STAGES[stage.slot_index()];
                StateRootApplyStageStats {
                    stage: stage.label(),
                    calls: slot.calls.load(Ordering::Relaxed),
                    avg_us: slot.avg_us.load(Ordering::Relaxed),
                }
            })
            .collect()
    }

    /// Returns local MPT apply item count metrics.
    pub fn state_root_apply_count_stats() -> Vec<StateRootApplyCountStats> {
        APPLY_COUNT_ORDER
            .iter()
            .map(|kind| {
                let slot = &APPLY_COUNTS[kind.slot_index()];
                StateRootApplyCountStats {
                    kind: kind.label(),
                    samples: slot.samples.load(Ordering::Relaxed),
                    total: slot.total.load(Ordering::Relaxed),
                    avg: slot.avg.load(Ordering::Relaxed),
                }
            })
            .collect()
    }
}

fn stage_avg(stage: StateRootApplyStage) -> u64 {
    APPLY_STAGES[stage.slot_index()]
        .avg_us
        .load(Ordering::Relaxed)
}

fn count_avg(kind: StateRootApplyCountKind) -> u64 {
    APPLY_COUNTS[kind.slot_index()].avg.load(Ordering::Relaxed)
}

fn ewma(slot: &AtomicU64, sample: u64) {
    let prev = slot.load(Ordering::Relaxed);
    let updated = if prev == 0 {
        sample
    } else {
        let diff = (sample as i64 - prev as i64) / 16;
        (prev as i64 + diff).max(0) as u64
    };
    slot.store(updated, Ordering::Relaxed);
}

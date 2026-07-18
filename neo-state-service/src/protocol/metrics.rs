use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of state root ingestion counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootIngestStats {
    /// Number of accepted state roots.
    pub accepted: u64,
    /// Number of rejected state roots.
    pub rejected: u64,
}

/// Snapshot of local StateService MPT apply counters, cumulative totals, and EWMA timings.
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
    /// Cumulative end-to-end local MPT apply time, in microseconds.
    pub total_us: u64,
    /// Cumulative snapshot-to-MPT changeset projection time, in microseconds.
    pub project_total_us: u64,
    /// Cumulative trie/write application time, in microseconds.
    pub apply_total_us: u64,
    /// Cumulative projected MPT changes across all apply attempts.
    pub changes_total: u64,
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
    /// Load and decode hash-addressed trie nodes from the current store snapshot.
    TrieResolveStore,
    /// Compute the new root hash after all mutations.
    RootHash,
    /// Commit dirty trie nodes into the MPT write batch.
    TrieCommit,
    /// Build and order the deferred full-state finalization lookup set.
    DeferredFinalizationPrepare,
    /// Resolve deferred full-state node bytes from the batch snapshot.
    DeferredFinalizationLookup,
    /// Resolve stored bytes and reference deltas into exact final node values.
    DeferredFinalizationResolve,
    /// Validate and split serialized deferred full-state nodes.
    DeferredFinalizationParse,
    /// Replay deferred reference operations against the resolved base node.
    DeferredFinalizationReplay,
    /// Encode deferred full-state node bytes with their final reference count.
    DeferredFinalizationEncode,
    /// Add the local state-root record and current-index record to the write batch.
    OverlayPrepare,
    /// Order the prepared MPT overlay by raw MDBX key before cursor writes.
    BackingSort,
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
            Self::TrieResolveStore => "trie_resolve_store",
            Self::RootHash => "root_hash",
            Self::TrieCommit => "trie_commit",
            Self::DeferredFinalizationPrepare => "deferred_finalization_prepare",
            Self::DeferredFinalizationLookup => "deferred_finalization_lookup",
            Self::DeferredFinalizationResolve => "deferred_finalization_resolve",
            Self::DeferredFinalizationParse => "deferred_finalization_parse",
            Self::DeferredFinalizationReplay => "deferred_finalization_replay",
            Self::DeferredFinalizationEncode => "deferred_finalization_encode",
            Self::OverlayPrepare => "overlay_prepare",
            Self::BackingSort => "backing_sort",
            Self::BackingCommit => "backing_commit",
            Self::PublishGeneration => "publish_generation",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::EnqueueBlocking => 0,
            Self::QueueWait => 1,
            Self::MutateChanges => 2,
            Self::TrieResolveStore => 3,
            Self::RootHash => 4,
            Self::TrieCommit => 5,
            Self::DeferredFinalizationPrepare => 6,
            Self::DeferredFinalizationLookup => 7,
            Self::DeferredFinalizationResolve => 8,
            Self::DeferredFinalizationParse => 9,
            Self::DeferredFinalizationReplay => 10,
            Self::DeferredFinalizationEncode => 11,
            Self::OverlayPrepare => 12,
            Self::BackingSort => 13,
            Self::BackingCommit => 14,
            Self::PublishGeneration => 15,
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
    /// Exact `0xf0 || hash` node puts in the prepared overlay.
    NodePuts,
    /// Exact `0xf0 || hash` node tombstones in the prepared overlay.
    NodeDeletes,
    /// Node put values from zero through 64 bytes.
    NodeValueSize0To64,
    /// Node put values from 65 through 128 bytes.
    NodeValueSize65To128,
    /// Node put values from 129 through 256 bytes.
    NodeValueSize129To256,
    /// Node put values from 257 through 512 bytes.
    NodeValueSize257To512,
    /// Node put values from 513 through 1,024 bytes.
    NodeValueSize513To1024,
    /// Node put values from 1,025 through 4,096 bytes.
    NodeValueSize1025To4096,
    /// Node put values from 4,097 through 16,384 bytes.
    NodeValueSize4097To16384,
    /// Node put values larger than 16,384 bytes.
    NodeValueSizeOver16384,
    /// Bytes in node put values from zero through 64 bytes.
    NodeValueBytes0To64,
    /// Bytes in node put values from 65 through 128 bytes.
    NodeValueBytes65To128,
    /// Bytes in node put values from 129 through 256 bytes.
    NodeValueBytes129To256,
    /// Bytes in node put values from 257 through 512 bytes.
    NodeValueBytes257To512,
    /// Bytes in node put values from 513 through 1,024 bytes.
    NodeValueBytes513To1024,
    /// Bytes in node put values from 1,025 through 4,096 bytes.
    NodeValueBytes1025To4096,
    /// Bytes in node put values from 4,097 through 16,384 bytes.
    NodeValueBytes4097To16384,
    /// Bytes in node put values larger than 16,384 bytes.
    NodeValueBytesOver16384,
    /// Nodes serialized and hashed through `MptCache::put_node_cached`.
    PutNodeCachedCalls,
    /// Serialized node payload bytes used as hash preimages.
    SerializedPayloadBytes,
    /// Actual node SHA-256 computations during mutation and root hashing.
    HashComputations,
    /// Maximum recursive mutation depth observed for one block.
    MaxRecursionDepth,
    /// Shared ancestors finalized repeatedly within one trie cache epoch.
    RepeatedAncestorFinalizations,
    /// Entries retained in the block-local write batch after a block commit.
    OverlayWorkingSetEntries,
    /// Finalized hashes already present in the current trie cache epoch.
    FinalizationCacheHits,
    /// Finalized hashes found in the mutable generation or write overlay.
    FinalizationMemoryHits,
    /// Finalized hashes absent from the mutable generation or write overlay.
    FinalizationMemoryMisses,
    /// Finalized hashes found in the frozen backing snapshot.
    FinalizationBackingHits,
    /// Finalized hashes absent from the frozen backing snapshot.
    FinalizationBackingMisses,
    /// Finalized hashes whose backing lookup returned an error.
    FinalizationLookupErrors,
    /// Entries exported in deferred full-state journals.
    DeferredJournalEntries,
    /// Trie hash resolutions served without consulting the store snapshot.
    TrieResolveCacheHits,
    /// Trie hash resolutions found in the store snapshot.
    TrieResolveStoreHits,
    /// Trie hash resolutions absent from the store snapshot.
    TrieResolveStoreMisses,
    /// Process-attributed physical read bytes observed around deferred
    /// full-state sorted lookups (best-effort on Linux).
    DeferredFinalizationReadBytes,
    /// Process minor page faults observed around deferred full-state sorted
    /// lookups (best-effort on Linux).
    DeferredFinalizationMinorFaults,
    /// Process major page faults observed around deferred full-state sorted
    /// lookups (best-effort on Linux).
    DeferredFinalizationMajorFaults,
}

impl StateRootApplyCountKind {
    fn label(self) -> &'static str {
        match self {
            Self::BatchBlocks => "batch_blocks",
            Self::Changes => "changes",
            Self::OverlayEntries => "overlay_entries",
            Self::OverlayPuts => "overlay_puts",
            Self::OverlayDeletes => "overlay_deletes",
            Self::NodePuts => "node_puts",
            Self::NodeDeletes => "node_deletes",
            Self::NodeValueSize0To64 => "node_value_size_0_64",
            Self::NodeValueSize65To128 => "node_value_size_65_128",
            Self::NodeValueSize129To256 => "node_value_size_129_256",
            Self::NodeValueSize257To512 => "node_value_size_257_512",
            Self::NodeValueSize513To1024 => "node_value_size_513_1024",
            Self::NodeValueSize1025To4096 => "node_value_size_1025_4096",
            Self::NodeValueSize4097To16384 => "node_value_size_4097_16384",
            Self::NodeValueSizeOver16384 => "node_value_size_over_16384",
            Self::NodeValueBytes0To64 => "node_value_bytes_0_64",
            Self::NodeValueBytes65To128 => "node_value_bytes_65_128",
            Self::NodeValueBytes129To256 => "node_value_bytes_129_256",
            Self::NodeValueBytes257To512 => "node_value_bytes_257_512",
            Self::NodeValueBytes513To1024 => "node_value_bytes_513_1024",
            Self::NodeValueBytes1025To4096 => "node_value_bytes_1025_4096",
            Self::NodeValueBytes4097To16384 => "node_value_bytes_4097_16384",
            Self::NodeValueBytesOver16384 => "node_value_bytes_over_16384",
            Self::PutNodeCachedCalls => "put_node_cached_calls",
            Self::SerializedPayloadBytes => "serialized_payload_bytes",
            Self::HashComputations => "hash_computations",
            Self::MaxRecursionDepth => "max_recursion_depth",
            Self::RepeatedAncestorFinalizations => "repeated_ancestor_finalizations",
            Self::OverlayWorkingSetEntries => "overlay_working_set_entries",
            Self::FinalizationCacheHits => "finalization_cache_hits",
            Self::FinalizationMemoryHits => "finalization_memory_hits",
            Self::FinalizationMemoryMisses => "finalization_memory_misses",
            Self::FinalizationBackingHits => "finalization_backing_hits",
            Self::FinalizationBackingMisses => "finalization_backing_misses",
            Self::FinalizationLookupErrors => "finalization_lookup_errors",
            Self::DeferredJournalEntries => "deferred_journal_entries",
            Self::TrieResolveCacheHits => "trie_resolve_cache_hits",
            Self::TrieResolveStoreHits => "trie_resolve_store_hits",
            Self::TrieResolveStoreMisses => "trie_resolve_store_misses",
            Self::DeferredFinalizationReadBytes => "deferred_finalization_read_bytes",
            Self::DeferredFinalizationMinorFaults => "deferred_finalization_minor_faults",
            Self::DeferredFinalizationMajorFaults => "deferred_finalization_major_faults",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::BatchBlocks => 0,
            Self::Changes => 1,
            Self::OverlayEntries => 2,
            Self::OverlayPuts => 3,
            Self::OverlayDeletes => 4,
            Self::NodePuts => 5,
            Self::NodeDeletes => 6,
            Self::NodeValueSize0To64 => 7,
            Self::NodeValueSize65To128 => 8,
            Self::NodeValueSize129To256 => 9,
            Self::NodeValueSize257To512 => 10,
            Self::NodeValueSize513To1024 => 11,
            Self::NodeValueSize1025To4096 => 12,
            Self::NodeValueSize4097To16384 => 13,
            Self::NodeValueSizeOver16384 => 14,
            Self::NodeValueBytes0To64 => 15,
            Self::NodeValueBytes65To128 => 16,
            Self::NodeValueBytes129To256 => 17,
            Self::NodeValueBytes257To512 => 18,
            Self::NodeValueBytes513To1024 => 19,
            Self::NodeValueBytes1025To4096 => 20,
            Self::NodeValueBytes4097To16384 => 21,
            Self::NodeValueBytesOver16384 => 22,
            Self::PutNodeCachedCalls => 23,
            Self::SerializedPayloadBytes => 24,
            Self::HashComputations => 25,
            Self::MaxRecursionDepth => 26,
            Self::RepeatedAncestorFinalizations => 27,
            Self::OverlayWorkingSetEntries => 28,
            Self::FinalizationCacheHits => 29,
            Self::FinalizationMemoryHits => 30,
            Self::FinalizationMemoryMisses => 31,
            Self::FinalizationBackingHits => 32,
            Self::FinalizationBackingMisses => 33,
            Self::FinalizationLookupErrors => 34,
            Self::DeferredJournalEntries => 35,
            Self::TrieResolveCacheHits => 36,
            Self::TrieResolveStoreHits => 37,
            Self::TrieResolveStoreMisses => 38,
            Self::DeferredFinalizationReadBytes => 39,
            Self::DeferredFinalizationMinorFaults => 40,
            Self::DeferredFinalizationMajorFaults => 41,
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
    /// Cumulative stage duration recorded since process start, in microseconds.
    pub total_us: u64,
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
    /// EWMA prepared-overlay sort duration in microseconds.
    pub backing_sort_avg_us: u64,
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
    total_us: AtomicU64,
    avg_scaled_us: AtomicU64,
}

impl TimingMetricSlot {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            avg_scaled_us: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct CountMetricSlot {
    samples: AtomicU64,
    total: AtomicU64,
    avg_scaled: AtomicU64,
}

impl CountMetricSlot {
    const fn new() -> Self {
        Self {
            samples: AtomicU64::new(0),
            total: AtomicU64::new(0),
            avg_scaled: AtomicU64::new(0),
        }
    }
}

static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static APPLY_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static APPLY_FAILURES: AtomicU64 = AtomicU64::new(0);
static APPLY_HEIGHT: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_TOTAL_SCALED_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_PROJECT_SCALED_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_APPLY_SCALED_US: AtomicU64 = AtomicU64::new(0);
static APPLY_AVG_CHANGES_SCALED: AtomicU64 = AtomicU64::new(0);
static APPLY_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static APPLY_PROJECT_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static APPLY_APPLY_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static APPLY_CHANGES_TOTAL: AtomicU64 = AtomicU64::new(0);
static APPLY_STAGE_ORDER: [StateRootApplyStage; 16] = [
    StateRootApplyStage::EnqueueBlocking,
    StateRootApplyStage::QueueWait,
    StateRootApplyStage::MutateChanges,
    StateRootApplyStage::TrieResolveStore,
    StateRootApplyStage::RootHash,
    StateRootApplyStage::TrieCommit,
    StateRootApplyStage::DeferredFinalizationPrepare,
    StateRootApplyStage::DeferredFinalizationLookup,
    StateRootApplyStage::DeferredFinalizationResolve,
    StateRootApplyStage::DeferredFinalizationParse,
    StateRootApplyStage::DeferredFinalizationReplay,
    StateRootApplyStage::DeferredFinalizationEncode,
    StateRootApplyStage::OverlayPrepare,
    StateRootApplyStage::BackingSort,
    StateRootApplyStage::BackingCommit,
    StateRootApplyStage::PublishGeneration,
];
static APPLY_STAGES: [TimingMetricSlot; 16] = [
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
    TimingMetricSlot::new(),
];
static APPLY_COUNT_ORDER: [StateRootApplyCountKind; 42] = [
    StateRootApplyCountKind::BatchBlocks,
    StateRootApplyCountKind::Changes,
    StateRootApplyCountKind::OverlayEntries,
    StateRootApplyCountKind::OverlayPuts,
    StateRootApplyCountKind::OverlayDeletes,
    StateRootApplyCountKind::NodePuts,
    StateRootApplyCountKind::NodeDeletes,
    StateRootApplyCountKind::NodeValueSize0To64,
    StateRootApplyCountKind::NodeValueSize65To128,
    StateRootApplyCountKind::NodeValueSize129To256,
    StateRootApplyCountKind::NodeValueSize257To512,
    StateRootApplyCountKind::NodeValueSize513To1024,
    StateRootApplyCountKind::NodeValueSize1025To4096,
    StateRootApplyCountKind::NodeValueSize4097To16384,
    StateRootApplyCountKind::NodeValueSizeOver16384,
    StateRootApplyCountKind::NodeValueBytes0To64,
    StateRootApplyCountKind::NodeValueBytes65To128,
    StateRootApplyCountKind::NodeValueBytes129To256,
    StateRootApplyCountKind::NodeValueBytes257To512,
    StateRootApplyCountKind::NodeValueBytes513To1024,
    StateRootApplyCountKind::NodeValueBytes1025To4096,
    StateRootApplyCountKind::NodeValueBytes4097To16384,
    StateRootApplyCountKind::NodeValueBytesOver16384,
    StateRootApplyCountKind::PutNodeCachedCalls,
    StateRootApplyCountKind::SerializedPayloadBytes,
    StateRootApplyCountKind::HashComputations,
    StateRootApplyCountKind::MaxRecursionDepth,
    StateRootApplyCountKind::RepeatedAncestorFinalizations,
    StateRootApplyCountKind::OverlayWorkingSetEntries,
    StateRootApplyCountKind::FinalizationCacheHits,
    StateRootApplyCountKind::FinalizationMemoryHits,
    StateRootApplyCountKind::FinalizationMemoryMisses,
    StateRootApplyCountKind::FinalizationBackingHits,
    StateRootApplyCountKind::FinalizationBackingMisses,
    StateRootApplyCountKind::FinalizationLookupErrors,
    StateRootApplyCountKind::DeferredJournalEntries,
    StateRootApplyCountKind::TrieResolveCacheHits,
    StateRootApplyCountKind::TrieResolveStoreHits,
    StateRootApplyCountKind::TrieResolveStoreMisses,
    StateRootApplyCountKind::DeferredFinalizationReadBytes,
    StateRootApplyCountKind::DeferredFinalizationMinorFaults,
    StateRootApplyCountKind::DeferredFinalizationMajorFaults,
];
static APPLY_COUNTS: [CountMetricSlot; 42] = [
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
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
        let has_previous_sample = APPLY_ATTEMPTS.fetch_add(1, Ordering::Relaxed) > 0;
        if !success {
            APPLY_FAILURES.fetch_add(1, Ordering::Relaxed);
        }
        APPLY_HEIGHT.store(block_index as u64, Ordering::Relaxed);
        APPLY_TOTAL_US.fetch_add(total_us, Ordering::Relaxed);
        APPLY_PROJECT_TOTAL_US.fetch_add(project_us, Ordering::Relaxed);
        APPLY_APPLY_TOTAL_US.fetch_add(apply_us, Ordering::Relaxed);
        APPLY_CHANGES_TOTAL.fetch_add(changes as u64, Ordering::Relaxed);
        ewma(&APPLY_AVG_TOTAL_SCALED_US, total_us, has_previous_sample);
        ewma(
            &APPLY_AVG_PROJECT_SCALED_US,
            project_us,
            has_previous_sample,
        );
        ewma(&APPLY_AVG_APPLY_SCALED_US, apply_us, has_previous_sample);
        ewma(
            &APPLY_AVG_CHANGES_SCALED,
            changes as u64,
            has_previous_sample,
        );
    }

    /// Records one fine-grained local MPT apply stage.
    pub fn record_stage(stage: StateRootApplyStage, elapsed_us: u64) {
        record_timing_slot(&APPLY_STAGES[stage.slot_index()], elapsed_us);
    }

    /// Records one local MPT apply item count.
    pub fn record_count(kind: StateRootApplyCountKind, count: u64) {
        record_count_slot(&APPLY_COUNTS[kind.slot_index()], count);
    }

    /// Returns the current local MPT apply counters and EWMA timings.
    pub fn state_root_apply_stats() -> StateRootApplyStats {
        StateRootApplyStats {
            attempts: APPLY_ATTEMPTS.load(Ordering::Relaxed),
            failures: APPLY_FAILURES.load(Ordering::Relaxed),
            latest_height: APPLY_HEIGHT.load(Ordering::Relaxed),
            avg_total_us: ewma_value(&APPLY_AVG_TOTAL_SCALED_US),
            avg_project_us: ewma_value(&APPLY_AVG_PROJECT_SCALED_US),
            avg_apply_us: ewma_value(&APPLY_AVG_APPLY_SCALED_US),
            avg_changes: ewma_value(&APPLY_AVG_CHANGES_SCALED),
            total_us: APPLY_TOTAL_US.load(Ordering::Relaxed),
            project_total_us: APPLY_PROJECT_TOTAL_US.load(Ordering::Relaxed),
            apply_total_us: APPLY_APPLY_TOTAL_US.load(Ordering::Relaxed),
            changes_total: APPLY_CHANGES_TOTAL.load(Ordering::Relaxed),
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
            backing_sort_avg_us: stage_avg(StateRootApplyStage::BackingSort),
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
                    total_us: slot.total_us.load(Ordering::Relaxed),
                    avg_us: ewma_value(&slot.avg_scaled_us),
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
                    avg: ewma_value(&slot.avg_scaled),
                }
            })
            .collect()
    }
}

fn stage_avg(stage: StateRootApplyStage) -> u64 {
    ewma_value(&APPLY_STAGES[stage.slot_index()].avg_scaled_us)
}

fn count_avg(kind: StateRootApplyCountKind) -> u64 {
    ewma_value(&APPLY_COUNTS[kind.slot_index()].avg_scaled)
}

// Retain fractional EWMA steps internally so small steady samples converge
// instead of sticking up to 15 whole units away from the observed value.
const EWMA_SCALE: u64 = 1 << 16;

fn record_timing_slot(slot: &TimingMetricSlot, elapsed_us: u64) {
    let has_previous_sample = slot.calls.fetch_add(1, Ordering::Relaxed) > 0;
    slot.total_us.fetch_add(elapsed_us, Ordering::Relaxed);
    ewma(&slot.avg_scaled_us, elapsed_us, has_previous_sample);
}

fn record_count_slot(slot: &CountMetricSlot, count: u64) {
    let has_previous_sample = slot.samples.fetch_add(1, Ordering::Relaxed) > 0;
    slot.total.fetch_add(count, Ordering::Relaxed);
    ewma(&slot.avg_scaled, count, has_previous_sample);
}

fn ewma(slot: &AtomicU64, sample: u64, has_previous_sample: bool) {
    let sample = sample.saturating_mul(EWMA_SCALE);
    let _ = slot.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
        Some(if !has_previous_sample {
            sample
        } else if sample >= prev {
            prev.saturating_add((sample - prev) / 16)
        } else {
            prev - (prev - sample) / 16
        })
    });
}

fn ewma_value(slot: &AtomicU64) -> u64 {
    slot.load(Ordering::Relaxed).saturating_add(EWMA_SCALE / 2) / EWMA_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_ewma_converges_inside_the_former_integer_dead_band() {
        let average = AtomicU64::new(0);
        ewma(&average, 32, false);
        for _ in 0..1_000 {
            ewma(&average, 17, true);
        }

        assert_eq!(ewma_value(&average), 17);
    }

    #[test]
    fn fixed_point_ewma_distinguishes_zero_from_an_uninitialized_slot() {
        let average = AtomicU64::new(0);
        ewma(&average, 0, false);
        ewma(&average, 160, true);

        assert_eq!(ewma_value(&average), 10);
    }

    #[test]
    fn metric_slots_retain_exact_cumulative_totals() {
        let timing = TimingMetricSlot::new();
        record_timing_slot(&timing, 19);
        record_timing_slot(&timing, 23);
        assert_eq!(timing.calls.load(Ordering::Relaxed), 2);
        assert_eq!(timing.total_us.load(Ordering::Relaxed), 42);

        let count = CountMetricSlot::new();
        record_count_slot(&count, 17);
        record_count_slot(&count, 25);
        assert_eq!(count.samples.load(Ordering::Relaxed), 2);
        assert_eq!(count.total.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn deferred_pack_metrics_are_in_the_bounded_registry() {
        let stages = StateRootApplyMetrics::state_root_apply_stage_stats();
        assert_eq!(stages.len(), 16);
        assert!(
            stages
                .iter()
                .any(|stat| stat.stage == "deferred_finalization_resolve")
        );
        assert!(stages.iter().any(|stat| stat.stage == "trie_resolve_store"));

        let counts = StateRootApplyMetrics::state_root_apply_count_stats();
        assert_eq!(counts.len(), 42);
        assert!(
            counts
                .iter()
                .any(|stat| stat.kind == "deferred_journal_entries")
        );
        for kind in [
            "trie_resolve_cache_hits",
            "trie_resolve_store_hits",
            "trie_resolve_store_misses",
            "deferred_finalization_read_bytes",
            "deferred_finalization_minor_faults",
            "deferred_finalization_major_faults",
        ] {
            assert!(counts.iter().any(|stat| stat.kind == kind));
        }
    }
}

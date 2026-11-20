//! Metrics for neo-node health and performance.
//!
//! Provides Prometheus-compatible metrics exposition.
//!
//! # Acceleration telemetry
//!
//! The acceleration structures below already compute their own statistics in
//! the crates that own them; this module only aggregates snapshots into the
//! exposition text and never derives a number that no counter produced:
//!
//! | Family prefix | Source |
//! | --- | --- |
//! | `neo_fast_path_*` | [`FastPathStatsSnapshot`] from `neo-system` |
//! | `neo_storage_read_cache_*`, `neo_storage_write_buffer_*` | [`StoreAccelerationStats`] from `neo-storage` |
//! | `neo_state_root_*` | `StateRootNodeCacheStats` from `neo-state-service` |
//! | `neo_mpt_stage_seconds_total` | [`ExecutionProfileSnapshot`] from `neo-system` |
//! | `neo_native_hook_*` | [`ExecutionProfileSnapshot`] native hook phases from `neo-system` |
//!
//! Every family uses bounded labels (fixed value sets declared by the enums
//! below), and every unit is one of counts, bytes, or seconds. Capturing a
//! snapshot only reads atomics and short-lived locks, so telemetry can never
//! change an execution or consensus decision.

use std::fmt::Display;
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use neo_storage::persistence::read_cache::ReadCacheStatsSnapshot;
use neo_storage::persistence::store::StoreAccelerationStats;
use neo_storage::persistence::write_batch_buffer::WriteBatchStatsSnapshot;
use neo_system::state_service::StateRootNodeCacheStats;
use neo_system::{
    ExecutionMode, ExecutionProfileSnapshot, FastPathStatsSnapshot, ParallelExecutionSnapshot,
};
use parking_lot::RwLock;

/// Simple telemetry collector for node metrics
pub struct Telemetry {
    name: String,
    version: String,
    block_height: AtomicU32,
    header_height: AtomicU32,
    verified_height: AtomicU32,
    mempool_verified: AtomicUsize,
    mempool_unverified: AtomicUsize,
    connection_count: AtomicUsize,
    timeout_count: AtomicU64,
    storage_free_gb: AtomicU64, // stored as u64 (GB * 1000)
    storage_total_gb: AtomicU64,
}

impl Telemetry {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            block_height: AtomicU32::new(0),
            header_height: AtomicU32::new(0),
            verified_height: AtomicU32::new(0),
            mempool_verified: AtomicUsize::new(0),
            mempool_unverified: AtomicUsize::new(0),
            connection_count: AtomicUsize::new(0),
            timeout_count: AtomicU64::new(0),
            storage_free_gb: AtomicU64::new(0),
            storage_total_gb: AtomicU64::new(0),
        }
    }

    pub fn update_heights(&self, block: u32, header: u32, verified: u32) {
        self.block_height.store(block, Ordering::Relaxed);
        self.header_height.store(header, Ordering::Relaxed);
        self.verified_height.store(verified, Ordering::Relaxed);
    }

    pub fn update_mempool(&self, verified: usize, unverified: usize) {
        self.mempool_verified.store(verified, Ordering::Relaxed);
        self.mempool_unverified.store(unverified, Ordering::Relaxed);
    }

    pub fn update_connections(&self, count: usize) {
        self.connection_count.store(count, Ordering::Relaxed);
    }

    pub fn update_timeouts(&self, count: u64) {
        self.timeout_count.store(count, Ordering::Relaxed);
    }

    pub fn update_storage(&self, free_gb: f64, total_gb: f64) {
        self.storage_free_gb
            .store((free_gb * 1000.0) as u64, Ordering::Relaxed);
        self.storage_total_gb
            .store((total_gb * 1000.0) as u64, Ordering::Relaxed);
    }

    pub fn export_prometheus(&self) -> String {
        let block = self.block_height.load(Ordering::Relaxed);
        let header = self.header_height.load(Ordering::Relaxed);
        let verified = self.verified_height.load(Ordering::Relaxed);
        let mempool_v = self.mempool_verified.load(Ordering::Relaxed);
        let mempool_u = self.mempool_unverified.load(Ordering::Relaxed);
        let connections = self.connection_count.load(Ordering::Relaxed);
        let timeouts = self.timeout_count.load(Ordering::Relaxed);
        let free = self.storage_free_gb.load(Ordering::Relaxed) as f64 / 1000.0;
        let total = self.storage_total_gb.load(Ordering::Relaxed) as f64 / 1000.0;

        format!(
            r#"# HELP neo_node_info Node information
# TYPE neo_node_info gauge
neo_node_info{{name="{}",version="{}"}} 1

# HELP neo_block_height Current block height
# TYPE neo_block_height gauge
neo_block_height {{}} {}

# HELP neo_header_height Current header height
# TYPE neo_header_height gauge
neo_header_height {{}} {}

# HELP neo_verified_height Current verified height
# TYPE neo_verified_height gauge
neo_verified_height {{}} {}

# HELP neo_mempool_verified Verified transactions in mempool
# TYPE neo_mempool_verified gauge
neo_mempool_verified {{}} {}

# HELP neo_mempool_unverified Unverified transactions in mempool
# TYPE neo_mempool_unverified gauge
neo_mempool_unverified {{}} {}

# HELP neo_connections Active P2P connections
# TYPE neo_connections gauge
neo_connections {{}} {}

# HELP neo_timeouts_total Total timeout count
# TYPE neo_timeouts_total counter
neo_timeouts_total {{}} {}

# HELP neo_storage_free_gb Free storage space in GB
# TYPE neo_storage_free_gb gauge
neo_storage_free_gb {{}} {:.2}

# HELP neo_storage_total_gb Total storage space in GB
# TYPE neo_storage_total_gb gauge
neo_storage_total_gb {{}} {:.2}
"#,
            self.name,
            self.version,
            block,
            header,
            verified,
            mempool_v,
            mempool_u,
            connections,
            timeouts,
            free,
            total
        )
    }
}

/// Direct-dispatch outcome buckets. Exactly two values, so the label is bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FastPathOutcome {
    /// Direct dispatch completed successfully.
    Halt,
    /// Direct dispatch ended in a canonical `FAULT`.
    Fault,
}

impl FastPathOutcome {
    /// Every exported outcome, in exposition order.
    pub const ALL: [Self; 2] = [Self::Halt, Self::Fault];

    /// Label value for `neo_fast_path_direct_outcomes_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Halt => "halt",
            Self::Fault => "fault",
        }
    }
}

/// Read-cache inserts rejected by the admission checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectedInsert {
    /// The entry alone exceeded the per-entry share of the byte budget.
    Oversized,
    /// The byte budget stayed exhausted after eviction.
    Budget,
}

impl RejectedInsert {
    /// Every exported rejection reason, in exposition order.
    pub const ALL: [Self; 2] = [Self::Oversized, Self::Budget];

    /// Label value for `neo_storage_read_cache_rejected_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::Budget => "budget",
        }
    }
}

/// MPT persistence stages with a dedicated timer in `neo-system`.
///
/// The stage set is fixed by `ExecutionStats`, so the `stage` label is bounded
/// by construction. These timings only accumulate while persistence profiling
/// is enabled; `neo_execution_profiling_enabled` reports whether they are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MptStage {
    /// Whole-trie update.
    Update,
    /// Trie commit into the node store.
    Commit,
    /// Root hash computation.
    RootHash,
    /// Node hashing (SHA-256 over encoded nodes).
    Hash,
    /// Node serialization before hashing.
    Serialization,
    /// Double SHA-256 hashing.
    Hash256,
    /// Node cache lookups.
    NodeCache,
    /// Node reads from the backing store.
    NodeRead,
    /// Node deserialization.
    NodeDeserialize,
    /// Node encoding.
    NodeEncode,
    /// Node writes.
    NodeWrite,
}

/// Number of exported MPT stages. Bounded by construction.
pub const MPT_STAGE_COUNT: usize = 11;

impl MptStage {
    /// Every exported stage, in exposition order.
    pub const ALL: [Self; MPT_STAGE_COUNT] = [
        Self::Update,
        Self::Commit,
        Self::RootHash,
        Self::Hash,
        Self::Serialization,
        Self::Hash256,
        Self::NodeCache,
        Self::NodeRead,
        Self::NodeDeserialize,
        Self::NodeEncode,
        Self::NodeWrite,
    ];

    /// Label value for `neo_mpt_stage_seconds_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Commit => "commit",
            Self::RootHash => "root_hash",
            Self::Hash => "hash",
            Self::Serialization => "serialization",
            Self::Hash256 => "hash256",
            Self::NodeCache => "node_cache",
            Self::NodeRead => "node_read",
            Self::NodeDeserialize => "node_deserialize",
            Self::NodeEncode => "node_encode",
            Self::NodeWrite => "node_write",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Update => 0,
            Self::Commit => 1,
            Self::RootHash => 2,
            Self::Hash => 3,
            Self::Serialization => 4,
            Self::Hash256 => 5,
            Self::NodeCache => 6,
            Self::NodeRead => 7,
            Self::NodeDeserialize => 8,
            Self::NodeEncode => 9,
            Self::NodeWrite => 10,
        }
    }

    /// Reads this stage's accumulated wall-clock time.
    pub fn seconds(self, profile: &ExecutionProfileSnapshot) -> f64 {
        match self {
            Self::Update => profile.mpt_update.as_secs_f64(),
            Self::Commit => profile.mpt_commit.as_secs_f64(),
            Self::RootHash => profile.mpt_root_hash.as_secs_f64(),
            Self::Hash => profile.mpt_hash.as_secs_f64(),
            Self::Serialization => profile.mpt_serialization.as_secs_f64(),
            Self::Hash256 => profile.mpt_hash256.as_secs_f64(),
            Self::NodeCache => profile.mpt_node_cache.as_secs_f64(),
            Self::NodeRead => profile.mpt_node_read.as_secs_f64(),
            Self::NodeDeserialize => profile.mpt_node_deserialize.as_secs_f64(),
            Self::NodeEncode => profile.mpt_node_encode.as_secs_f64(),
            Self::NodeWrite => profile.mpt_node_write.as_secs_f64(),
        }
    }
}

/// Block-level native hook phases timed by the persistence profiler.
///
/// The profiler records one duration per phase for each persisted block, so the
/// `phase` label is bounded by construction. Per-contract hook timings have no
/// production data source and are deliberately not exported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHookPhase {
    /// `OnPersist` dispatch for the block.
    OnPersist,
    /// `PostPersist` dispatch for the block.
    PostPersist,
}

/// Number of exported native hook phases. Bounded by construction.
pub const NATIVE_HOOK_PHASE_COUNT: usize = 2;

impl NativeHookPhase {
    /// Every exported phase, in exposition order.
    pub const ALL: [Self; NATIVE_HOOK_PHASE_COUNT] = [Self::OnPersist, Self::PostPersist];

    /// Label value for `neo_native_hook_seconds_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OnPersist => "on_persist",
            Self::PostPersist => "post_persist",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::OnPersist => 0,
            Self::PostPersist => 1,
        }
    }
}

/// One observation pass over every acceleration structure the node owns.
///
/// Each `Option` field mirrors the store's answer: `None` means the component
/// is not running in this build, which the exposition reports as zero with the
/// matching `*_enabled` gauge at zero instead of omitting the family.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccelerationSnapshot {
    /// Fast-path counters already classified by the execution strategy.
    pub fast_path: FastPathStatsSnapshot,
    /// Speculative parallel transaction scheduler counters.
    pub parallel: ParallelExecutionSnapshot,
    /// Execution mode selected for block persistence.
    pub execution_mode: ExecutionMode,
    /// Point read cache counters, when the active store runs one.
    pub read_cache: Option<ReadCacheStatsSnapshot>,
    /// Write-batch buffer counters, when the active store buffers writes.
    pub write_buffer: Option<WriteBatchStatsSnapshot>,
    /// State-root node cache counters, when the state service is registered.
    pub state_root_node_cache: Option<StateRootNodeCacheStats>,
    /// Whether persistence profiling is currently accumulating MPT timings.
    pub profiling_enabled: bool,
    /// Per-stage MPT wall-clock seconds, indexed by `MptStage::index`.
    pub mpt_seconds: [f64; MPT_STAGE_COUNT],
    /// Native hook seconds across all blocks, indexed by `NativeHookPhase::index`.
    pub native_hook_seconds: [f64; NATIVE_HOOK_PHASE_COUNT],
    /// The empty-block subset of [`Self::native_hook_seconds`].
    pub native_hook_empty_block_seconds: [f64; NATIVE_HOOK_PHASE_COUNT],
}

impl AccelerationSnapshot {
    /// Maps lower-crate snapshots into the exported shape.
    ///
    /// This is a pure mapping: it reads the given snapshots and performs no
    /// evaluation, caching, or persistence work.
    pub fn capture(
        fast_path: FastPathStatsSnapshot,
        parallel: ParallelExecutionSnapshot,
        execution_mode: ExecutionMode,
        profile: &ExecutionProfileSnapshot,
        acceleration: Option<&StoreAccelerationStats>,
        state_root_node_cache: Option<StateRootNodeCacheStats>,
    ) -> Self {
        let mut mpt_seconds = [0.0; MPT_STAGE_COUNT];
        for stage in MptStage::ALL {
            mpt_seconds[stage.index()] = stage.seconds(profile);
        }
        let mut native_hook_seconds = [0.0; NATIVE_HOOK_PHASE_COUNT];
        let mut native_hook_empty_block_seconds = [0.0; NATIVE_HOOK_PHASE_COUNT];
        for phase in NativeHookPhase::ALL {
            let (all_blocks, empty_blocks) = match phase {
                NativeHookPhase::OnPersist => (profile.on_persist, profile.empty_blocks.on_persist),
                NativeHookPhase::PostPersist => {
                    (profile.post_persist, profile.empty_blocks.post_persist)
                }
            };
            native_hook_seconds[phase.index()] = all_blocks.as_secs_f64();
            native_hook_empty_block_seconds[phase.index()] = empty_blocks.as_secs_f64();
        }
        Self {
            fast_path,
            parallel,
            execution_mode,
            read_cache: acceleration.and_then(|stats| stats.read_cache),
            write_buffer: acceleration.and_then(|stats| stats.write_buffer),
            state_root_node_cache,
            profiling_enabled: profile.enabled,
            mpt_seconds,
            native_hook_seconds,
            native_hook_empty_block_seconds,
        }
    }

    /// Reads one MPT stage from the snapshot.
    pub fn mpt_stage_seconds(&self, stage: MptStage) -> f64 {
        self.mpt_seconds[stage.index()]
    }

    /// Reads one native hook phase's seconds across all blocks.
    pub fn native_hook_seconds(&self, phase: NativeHookPhase) -> f64 {
        self.native_hook_seconds[phase.index()]
    }

    /// Reads the empty-block subset of one native hook phase.
    pub fn native_hook_empty_block_seconds(&self, phase: NativeHookPhase) -> f64 {
        self.native_hook_empty_block_seconds[phase.index()]
    }

    fn direct_outcome(&self, outcome: FastPathOutcome) -> u64 {
        match outcome {
            FastPathOutcome::Halt => self.fast_path.direct_halts,
            FastPathOutcome::Fault => self.fast_path.direct_faults,
        }
    }

    fn rejected_insert(&self, reason: RejectedInsert) -> u64 {
        let Some(cache) = self.read_cache.as_ref() else {
            return 0;
        };
        match reason {
            RejectedInsert::Oversized => cache.rejected_oversized,
            RejectedInsert::Budget => cache.rejected_budget,
        }
    }

    fn read_cache_or_default(&self) -> ReadCacheStatsSnapshot {
        self.read_cache.unwrap_or_default()
    }

    fn write_buffer_or_default(&self) -> WriteBatchStatsSnapshot {
        self.write_buffer.unwrap_or_default()
    }

    fn state_root_enabled(&self) -> bool {
        self.state_root_node_cache
            .is_some_and(|stats| stats.enabled)
    }

    /// Renders the acceleration families as Prometheus exposition text.
    pub fn export_prometheus(&self) -> String {
        let mut out = String::with_capacity(4 * 1024);
        self.write_prometheus(&mut out);
        out
    }

    fn write_prometheus(&self, out: &mut String) {
        self.write_fast_path(out);
        self.write_parallel(out);
        self.write_read_cache(out);
        self.write_write_buffer(out);
        self.write_state_root(out);
        self.write_mpt(out);
        self.write_native_hooks(out);
    }

    /// Renders the speculative scheduler families.
    ///
    /// All values are counters the scheduler maintained while persisting blocks;
    /// they describe work already decided and never feed back into execution.
    fn write_parallel(&self, out: &mut String) {
        header(
            out,
            "neo_parallel_speculative_blocks_total",
            "counter",
            "Blocks whose transactions were executed speculatively before validation",
        );
        sample(
            out,
            "neo_parallel_speculative_blocks_total",
            "",
            self.parallel.speculative_blocks,
        );

        header(
            out,
            "neo_parallel_speculative_transactions_total",
            "counter",
            "Transactions executed speculatively, including runs later discarded",
        );
        sample(
            out,
            "neo_parallel_speculative_transactions_total",
            "",
            self.parallel.speculative_transactions,
        );

        header(
            out,
            "neo_parallel_conflicts_total",
            "counter",
            "Speculative runs rejected by ordered validation",
        );
        sample(
            out,
            "neo_parallel_conflicts_total",
            "",
            self.parallel.conflicts,
        );

        header(
            out,
            "neo_parallel_reexecutions_total",
            "counter",
            "Transactions re-executed serially after a detected conflict",
        );
        sample(
            out,
            "neo_parallel_reexecutions_total",
            "",
            self.parallel.reexecutions,
        );

        header(
            out,
            "neo_parallel_serial_fallbacks_total",
            "counter",
            "Blocks routed to the serial loop while parallel execution was enabled",
        );
        sample(
            out,
            "neo_parallel_serial_fallbacks_total",
            "",
            self.parallel.serial_fallbacks,
        );

        header(
            out,
            "neo_parallel_seconds_total",
            "counter",
            "Wall-clock seconds spent in the speculative scheduler by phase",
        );
        sample(
            out,
            "neo_parallel_seconds_total",
            "{phase=\"speculative\"}",
            nanos_to_seconds(self.parallel.parallel_nanos),
        );
        sample(
            out,
            "neo_parallel_seconds_total",
            "{phase=\"serial\"}",
            nanos_to_seconds(self.parallel.serial_nanos),
        );
    }

    fn write_fast_path(&self, out: &mut String) {
        header(
            out,
            "neo_fast_path_direct_attempts_total",
            "counter",
            "Direct native NEP-17 dispatches that ran, including faulted ones",
        );
        sample(
            out,
            "neo_fast_path_direct_attempts_total",
            "",
            self.fast_path.direct_attempts,
        );

        header(
            out,
            "neo_fast_path_direct_outcomes_total",
            "counter",
            "Direct native NEP-17 dispatches by final VM state",
        );
        for outcome in FastPathOutcome::ALL {
            sample(
                out,
                "neo_fast_path_direct_outcomes_total",
                &format!("{{outcome=\"{}\"}}", outcome.label()),
                self.direct_outcome(outcome),
            );
        }

        header(
            out,
            "neo_fast_path_eligibility_declines_total",
            "counter",
            "Parsed transfers rejected by the engine-side eligibility checks",
        );
        sample(
            out,
            "neo_fast_path_eligibility_declines_total",
            "",
            self.fast_path.eligibility_declines,
        );

        header(
            out,
            "neo_fast_path_canonical_fallbacks_total",
            "counter",
            "Verify-mode executions that fell back to the canonical interpreter",
        );
        sample(
            out,
            "neo_fast_path_canonical_fallbacks_total",
            "",
            self.fast_path.canonical_fallbacks,
        );

        header(
            out,
            "neo_fast_path_verification_mismatches_total",
            "counter",
            "Dual executions whose direct and canonical fingerprints diverged",
        );
        sample(
            out,
            "neo_fast_path_verification_mismatches_total",
            "",
            self.fast_path.verification_mismatches,
        );

        header(
            out,
            "neo_fast_path_mode",
            "gauge",
            "Selected execution mode, one-hot: 1 for the active mode",
        );
        let active = self.execution_mode.as_str();
        for mode in ExecutionMode::ALL {
            let value = u8::from(mode.as_str() == active);
            sample(
                out,
                "neo_fast_path_mode",
                &format!("{{mode=\"{}\"}}", mode.as_str()),
                value,
            );
        }
    }

    fn write_read_cache(&self, out: &mut String) {
        header(
            out,
            "neo_storage_acceleration_enabled",
            "gauge",
            "Whether the active store runs the named acceleration component and it is producing telemetry",
        );
        sample(
            out,
            "neo_storage_acceleration_enabled",
            "{component=\"read_cache\"}",
            u8::from(self.read_cache.is_some()),
        );
        sample(
            out,
            "neo_storage_acceleration_enabled",
            "{component=\"write_buffer\"}",
            u8::from(self.write_buffer.is_some()),
        );

        let cache = self.read_cache_or_default();

        header(
            out,
            "neo_storage_read_cache_hits_total",
            "counter",
            "Read cache lookups served from resident entries",
        );
        sample(out, "neo_storage_read_cache_hits_total", "", cache.hits);

        header(
            out,
            "neo_storage_read_cache_misses_total",
            "counter",
            "Read cache lookups that found no resident entry",
        );
        sample(out, "neo_storage_read_cache_misses_total", "", cache.misses);

        header(
            out,
            "neo_storage_read_cache_evictions_total",
            "counter",
            "Read cache entries removed by LRU or TTL expiry",
        );
        sample(
            out,
            "neo_storage_read_cache_evictions_total",
            "",
            cache.evictions,
        );

        header(
            out,
            "neo_storage_read_cache_admissions_total",
            "counter",
            "Read cache entries accepted by the admission checks",
        );
        sample(
            out,
            "neo_storage_read_cache_admissions_total",
            "",
            cache.inserts,
        );

        header(
            out,
            "neo_storage_read_cache_rejected_total",
            "counter",
            "Read cache inserts rejected by the admission checks",
        );
        for reason in RejectedInsert::ALL {
            sample(
                out,
                "neo_storage_read_cache_rejected_total",
                &format!("{{reason=\"{}\"}}", reason.label()),
                self.rejected_insert(reason),
            );
        }

        header(
            out,
            "neo_storage_read_cache_prefetches_total",
            "counter",
            "Speculative values offered to the read cache by prefetch",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetches_total",
            "",
            cache.prefetches,
        );

        header(
            out,
            "neo_storage_read_cache_prefetch_hits_total",
            "counter",
            "Read cache hits served by a prefetched entry",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetch_hits_total",
            "",
            cache.prefetch_hits,
        );

        header(
            out,
            "neo_storage_read_cache_prefetch_stale_skips_total",
            "counter",
            "Prefetched values dropped because a fresher entry was resident",
        );
        sample(
            out,
            "neo_storage_read_cache_prefetch_stale_skips_total",
            "",
            cache.prefetch_stale_skips,
        );

        header(
            out,
            "neo_storage_read_cache_bloom_checks_total",
            "counter",
            "Read cache lookups that probed the bloom filter",
        );
        sample(
            out,
            "neo_storage_read_cache_bloom_checks_total",
            "",
            cache.bloom_filter_checks,
        );

        header(
            out,
            "neo_storage_read_cache_bloom_negatives_total",
            "counter",
            "Bloom filter probes that proved the key absent without a lookup",
        );
        sample(
            out,
            "neo_storage_read_cache_bloom_negatives_total",
            "",
            cache.bloom_filter_negatives,
        );

        header(
            out,
            "neo_storage_read_cache_entries",
            "gauge",
            "Resident read cache entries",
        );
        sample(
            out,
            "neo_storage_read_cache_entries",
            "",
            cache.current_entries,
        );

        header(
            out,
            "neo_storage_read_cache_bytes",
            "gauge",
            "Resident read cache payload bytes",
        );
        sample(out, "neo_storage_read_cache_bytes", "", cache.current_bytes);
    }

    fn write_write_buffer(&self, out: &mut String) {
        let buffer = self.write_buffer_or_default();

        header(
            out,
            "neo_storage_write_buffer_flushes_total",
            "counter",
            "Write-batch buffer flushes completed",
        );
        sample(
            out,
            "neo_storage_write_buffer_flushes_total",
            "",
            buffer.batches_flushed,
        );

        header(
            out,
            "neo_storage_write_buffer_operations_written_total",
            "counter",
            "Store operations written by buffer flushes",
        );
        sample(
            out,
            "neo_storage_write_buffer_operations_written_total",
            "",
            buffer.operations_written,
        );

        header(
            out,
            "neo_storage_write_buffer_bytes_written_total",
            "counter",
            "Bytes written by buffer flushes",
        );
        sample(
            out,
            "neo_storage_write_buffer_bytes_written_total",
            "",
            buffer.bytes_written,
        );

        header(
            out,
            "neo_storage_write_buffer_flush_seconds_total",
            "counter",
            "Cumulative buffer flush wall-clock time in seconds",
        );
        sample(
            out,
            "neo_storage_write_buffer_flush_seconds_total",
            "",
            buffer.total_flush_duration_ms as f64 / 1000.0,
        );

        header(
            out,
            "neo_storage_write_buffer_flush_timeouts_total",
            "counter",
            "Buffer flushes triggered by the delay timeout",
        );
        sample(
            out,
            "neo_storage_write_buffer_flush_timeouts_total",
            "",
            buffer.flush_timeouts,
        );

        header(
            out,
            "neo_storage_write_buffer_pending_operations",
            "gauge",
            "Operations buffered and not yet flushed",
        );
        sample(
            out,
            "neo_storage_write_buffer_pending_operations",
            "",
            buffer.pending_operations,
        );

        header(
            out,
            "neo_storage_write_buffer_pending_bytes",
            "gauge",
            "Payload bytes buffered and not yet flushed",
        );
        sample(
            out,
            "neo_storage_write_buffer_pending_bytes",
            "",
            buffer.pending_bytes,
        );
    }

    fn write_state_root(&self, out: &mut String) {
        let cache = self.state_root_node_cache.unwrap_or_default();

        header(
            out,
            "neo_state_root_node_cache_enabled",
            "gauge",
            "Whether the state-root node cache is active",
        );
        sample(
            out,
            "neo_state_root_node_cache_enabled",
            "",
            u8::from(self.state_root_enabled()),
        );

        header(
            out,
            "neo_state_root_node_cache_hits_total",
            "counter",
            "State-root MPT node lookups served from the shared node cache",
        );
        sample(out, "neo_state_root_node_cache_hits_total", "", cache.hits);

        header(
            out,
            "neo_state_root_node_cache_misses_total",
            "counter",
            "State-root MPT node lookups that missed the shared node cache",
        );
        sample(
            out,
            "neo_state_root_node_cache_misses_total",
            "",
            cache.misses,
        );

        header(
            out,
            "neo_state_root_node_cache_entries",
            "gauge",
            "Resident state-root node cache entries",
        );
        sample(out, "neo_state_root_node_cache_entries", "", cache.entries);

        header(
            out,
            "neo_state_root_node_cache_capacity",
            "gauge",
            "Configured state-root node cache capacity in entries",
        );
        sample(
            out,
            "neo_state_root_node_cache_capacity",
            "",
            cache.capacity,
        );

        header(
            out,
            "neo_state_root_warm_root_nodes",
            "gauge",
            "Resolved nodes retained for the warm state root, excluding hash stubs",
        );
        sample(
            out,
            "neo_state_root_warm_root_nodes",
            "",
            cache.warm_root_nodes,
        );

        header(
            out,
            "neo_state_root_warm_root_bytes",
            "gauge",
            "Estimated bytes retained for the warm state root",
        );
        sample(
            out,
            "neo_state_root_warm_root_bytes",
            "",
            cache.warm_root_estimated_bytes,
        );
    }

    fn write_mpt(&self, out: &mut String) {
        header(
            out,
            "neo_execution_profiling_enabled",
            "gauge",
            "Whether persistence profiling is accumulating MPT timings",
        );
        sample(
            out,
            "neo_execution_profiling_enabled",
            "",
            u8::from(self.profiling_enabled),
        );

        header(
            out,
            "neo_mpt_stage_seconds_total",
            "counter",
            "Cumulative MPT persistence stage wall-clock time in seconds",
        );
        for stage in MptStage::ALL {
            sample(
                out,
                "neo_mpt_stage_seconds_total",
                &format!("{{stage=\"{}\"}}", stage.label()),
                self.mpt_stage_seconds(stage),
            );
        }
    }

    fn write_native_hooks(&self, out: &mut String) {
        header(
            out,
            "neo_native_hook_seconds_total",
            "counter",
            "Cumulative native OnPersist and PostPersist wall-clock time in seconds",
        );
        for phase in NativeHookPhase::ALL {
            sample(
                out,
                "neo_native_hook_seconds_total",
                &format!("{{phase=\"{}\"}}", phase.label()),
                self.native_hook_seconds(phase),
            );
        }

        header(
            out,
            "neo_native_hook_empty_block_seconds_total",
            "counter",
            "The empty-block subset of neo_native_hook_seconds_total",
        );
        for phase in NativeHookPhase::ALL {
            sample(
                out,
                "neo_native_hook_empty_block_seconds_total",
                &format!("{{phase=\"{}\"}}", phase.label()),
                self.native_hook_empty_block_seconds(phase),
            );
        }
    }
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn sample(out: &mut String, name: &str, labels: &str, value: impl Display) {
    let _ = writeln!(out, "{name}{labels} {value}");
}

/// Converts accumulated nanoseconds into the seconds a Prometheus counter expects.
fn nanos_to_seconds(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000_000.0
}

/// Global acceleration snapshot, refreshed by the metrics pump.
static ACCELERATION: LazyLock<RwLock<AccelerationSnapshot>> =
    LazyLock::new(|| RwLock::new(AccelerationSnapshot::default()));

/// Replaces the exported acceleration snapshot.
pub fn update_acceleration_metrics(snapshot: AccelerationSnapshot) {
    *ACCELERATION.write() = snapshot;
}

/// Returns the currently exported acceleration snapshot.
pub fn acceleration_snapshot() -> AccelerationSnapshot {
    *ACCELERATION.read()
}

/// Global telemetry instance for the node
pub static TELEMETRY: LazyLock<Arc<Telemetry>> =
    LazyLock::new(|| Arc::new(Telemetry::new("neo-node", env!("CARGO_PKG_VERSION"))));

/// Returns the global telemetry instance
#[allow(dead_code)]
pub fn telemetry() -> &'static Arc<Telemetry> {
    &TELEMETRY
}

/// Updates all node metrics
pub fn update_node_metrics(
    block_height: u32,
    header_height: u32,
    verified_height: u32,
    mempool_verified: usize,
    mempool_unverified: usize,
    connection_count: usize,
) {
    let tel = telemetry();
    tel.update_heights(block_height, header_height, verified_height);
    tel.update_mempool(mempool_verified, mempool_unverified);
    tel.update_connections(connection_count);
}

/// Updates timeout metrics
pub fn update_timeout_metrics(_peer_count: usize, timeout_count: usize) {
    telemetry().update_timeouts(timeout_count as u64);
}

/// Updates storage metrics
pub fn update_storage_metrics(free_gb: f64, total_gb: f64) {
    telemetry().update_storage(free_gb, total_gb);
}

/// Gathers Prometheus metrics
pub fn gather_prometheus() -> Vec<u8> {
    let mut text = telemetry().export_prometheus();
    text.push_str(&acceleration_snapshot().export_prometheus());
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Every label key the acceleration surface is allowed to emit.
    const LABEL_SCHEMA: &[(&str, &[&str])] = &[
        ("outcome", &["halt", "fault"]),
        ("reason", &["oversized", "budget"]),
        ("mode", &["canonical", "auto", "verify"]),
        ("component", &["read_cache", "write_buffer"]),
        (
            "phase",
            &["on_persist", "post_persist", "speculative", "serial"],
        ),
        (
            "stage",
            &[
                "update",
                "commit",
                "root_hash",
                "hash",
                "serialization",
                "hash256",
                "node_cache",
                "node_read",
                "node_deserialize",
                "node_encode",
                "node_write",
            ],
        ),
    ];

    fn populated_snapshot() -> AccelerationSnapshot {
        let fast_path = FastPathStatsSnapshot {
            direct_attempts: 11,
            direct_halts: 9,
            direct_faults: 2,
            eligibility_declines: 4,
            canonical_fallbacks: 3,
            verification_mismatches: 1,
        };
        let mut profile = ExecutionProfileSnapshot {
            enabled: true,
            mpt_update: Duration::from_millis(1500),
            mpt_hash: Duration::from_millis(250),
            mpt_hash256: Duration::from_micros(500),
            on_persist: Duration::from_millis(2000),
            post_persist: Duration::from_millis(500),
            ..ExecutionProfileSnapshot::default()
        };
        profile.empty_blocks.on_persist = Duration::from_millis(250);
        profile.empty_blocks.post_persist = Duration::from_millis(50);
        let acceleration = StoreAccelerationStats {
            read_cache: Some(ReadCacheStatsSnapshot {
                hits: 100,
                misses: 20,
                evictions: 7,
                prefetches: 30,
                prefetch_hits: 25,
                inserts: 90,
                current_entries: 42,
                current_bytes: 4096,
                rejected_oversized: 5,
                rejected_budget: 2,
                prefetch_stale_skips: 6,
                bloom_filter_checks: 40,
                bloom_filter_negatives: 12,
            }),
            write_buffer: Some(WriteBatchStatsSnapshot {
                batches_flushed: 8,
                operations_written: 800,
                bytes_written: 65536,
                total_flush_duration_ms: 4000,
                flush_timeouts: 1,
                pending_operations: 12,
                pending_bytes: 2048,
            }),
        };
        let state_root = StateRootNodeCacheStats {
            enabled: true,
            capacity: 65536,
            entries: 512,
            hits: 64,
            misses: 16,
            hit_rate_percent: 80.0,
            warm_root_nodes: 300,
            warm_root_estimated_bytes: 98304,
        };
        AccelerationSnapshot::capture(
            fast_path,
            ParallelExecutionSnapshot::default(),
            ExecutionMode::Auto,
            &profile,
            Some(&acceleration),
            Some(state_root),
        )
    }

    /// Collects `(metric_name, label_key, label_value)` for every labelled sample.
    fn labelled_samples(text: &str) -> Vec<(String, String, String)> {
        let mut found = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || !line.contains('{') {
                continue;
            }
            let (name, rest) = line.split_once('{').expect("labelled sample");
            let (labels, _) = rest.split_once('}').expect("closed label set");
            for pair in labels.split(',') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                let (key, value) = pair.split_once('=').expect("key=value label");
                found.push((
                    name.to_string(),
                    key.to_string(),
                    value.trim_matches('"').to_string(),
                ));
            }
        }
        found
    }

    fn value_of(text: &str, sample_line: &str) -> f64 {
        text.lines()
            .find(|line| line.starts_with(sample_line))
            .unwrap_or_else(|| panic!("missing sample `{sample_line}`"))
            .rsplit_once(' ')
            .expect("sample has a value")
            .1
            .parse()
            .expect("numeric sample value")
    }

    #[test]
    fn telemetry_is_accessible() {
        let _ = telemetry();
    }

    #[test]
    fn update_metrics_exports_prometheus_format() {
        update_node_metrics(100, 100, 100, 50, 10, 5);
        let exported = gather_prometheus();
        let text = String::from_utf8(exported).unwrap();

        assert!(text.contains("neo_block_height"));
        assert!(text.contains("neo_mempool_verified"));
        assert!(text.contains("100"));
    }

    #[test]
    fn storage_metrics_work() {
        update_storage_metrics(50.5, 100.0);
        let exported = gather_prometheus();
        let text = String::from_utf8(exported).unwrap();

        assert!(text.contains("neo_storage_free_gb"));
        assert!(text.contains("50."));
    }

    #[test]
    fn mpt_stage_table_covers_every_exported_stage() {
        assert_eq!(MptStage::ALL.len(), MPT_STAGE_COUNT);
        let mut labels: Vec<&str> = MptStage::ALL.iter().map(|stage| stage.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), MPT_STAGE_COUNT, "stage labels are unique");
    }

    #[test]
    fn native_hook_phase_table_is_bounded() {
        assert_eq!(NativeHookPhase::ALL.len(), NATIVE_HOOK_PHASE_COUNT);
        let mut labels: Vec<&str> = NativeHookPhase::ALL
            .iter()
            .map(|phase| phase.label())
            .collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            NATIVE_HOOK_PHASE_COUNT,
            "phase labels are unique"
        );
        let mut indexes: Vec<usize> = NativeHookPhase::ALL
            .iter()
            .map(|phase| phase.index())
            .collect();
        indexes.sort_unstable();
        assert_eq!(indexes, (0..NATIVE_HOOK_PHASE_COUNT).collect::<Vec<_>>());
    }

    #[test]
    fn acceleration_metric_names_are_stable() {
        let text = populated_snapshot().export_prometheus();
        for name in [
            "neo_fast_path_direct_attempts_total",
            "neo_fast_path_direct_outcomes_total",
            "neo_fast_path_eligibility_declines_total",
            "neo_fast_path_canonical_fallbacks_total",
            "neo_fast_path_verification_mismatches_total",
            "neo_fast_path_mode",
            "neo_parallel_speculative_blocks_total",
            "neo_parallel_speculative_transactions_total",
            "neo_parallel_conflicts_total",
            "neo_parallel_reexecutions_total",
            "neo_parallel_serial_fallbacks_total",
            "neo_parallel_seconds_total",
            "neo_storage_acceleration_enabled",
            "neo_storage_read_cache_hits_total",
            "neo_storage_read_cache_misses_total",
            "neo_storage_read_cache_evictions_total",
            "neo_storage_read_cache_admissions_total",
            "neo_storage_read_cache_rejected_total",
            "neo_storage_read_cache_prefetches_total",
            "neo_storage_read_cache_prefetch_hits_total",
            "neo_storage_read_cache_prefetch_stale_skips_total",
            "neo_storage_read_cache_bloom_checks_total",
            "neo_storage_read_cache_bloom_negatives_total",
            "neo_storage_read_cache_entries",
            "neo_storage_read_cache_bytes",
            "neo_storage_write_buffer_flushes_total",
            "neo_storage_write_buffer_operations_written_total",
            "neo_storage_write_buffer_bytes_written_total",
            "neo_storage_write_buffer_flush_seconds_total",
            "neo_storage_write_buffer_flush_timeouts_total",
            "neo_storage_write_buffer_pending_operations",
            "neo_storage_write_buffer_pending_bytes",
            "neo_state_root_node_cache_enabled",
            "neo_state_root_node_cache_hits_total",
            "neo_state_root_node_cache_misses_total",
            "neo_state_root_node_cache_entries",
            "neo_state_root_node_cache_capacity",
            "neo_state_root_warm_root_nodes",
            "neo_state_root_warm_root_bytes",
            "neo_execution_profiling_enabled",
            "neo_mpt_stage_seconds_total",
            "neo_native_hook_seconds_total",
            "neo_native_hook_empty_block_seconds_total",
        ] {
            assert!(
                text.contains(&format!("# TYPE {name} ")),
                "missing HELP/TYPE metadata for {name}"
            );
        }
    }

    #[test]
    fn acceleration_labels_are_bounded() {
        let text = populated_snapshot().export_prometheus();
        let samples = labelled_samples(&text);
        assert!(!samples.is_empty(), "expected labelled samples");

        for (name, key, value) in samples {
            let Some((_, allowed)) = LABEL_SCHEMA.iter().find(|(known, _)| *known == key) else {
                panic!("unbounded label key `{key}` on `{name}`");
            };
            assert!(
                allowed.contains(&value.as_str()),
                "label `{key}=\"{value}\"` on `{name}` is outside the bounded set"
            );
        }
    }

    #[test]
    fn acceleration_families_have_no_label_collisions() {
        let text = populated_snapshot().export_prometheus();
        let mut seen = std::collections::BTreeSet::new();
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let series = line.rsplit_once(' ').expect("sample line").0;
            assert!(seen.insert(series.to_string()), "duplicate series {series}");
        }
    }

    #[test]
    fn snapshot_maps_counters_to_samples() {
        let text = populated_snapshot().export_prometheus();

        assert_eq!(value_of(&text, "neo_fast_path_direct_attempts_total"), 11.0);
        assert_eq!(
            value_of(
                &text,
                "neo_fast_path_direct_outcomes_total{outcome=\"halt\"}"
            ),
            9.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_fast_path_direct_outcomes_total{outcome=\"fault\"}"
            ),
            2.0
        );
        assert_eq!(
            value_of(&text, "neo_fast_path_eligibility_declines_total"),
            4.0
        );
        assert_eq!(
            value_of(&text, "neo_fast_path_canonical_fallbacks_total"),
            3.0
        );
        assert_eq!(
            value_of(&text, "neo_fast_path_verification_mismatches_total"),
            1.0
        );

        assert_eq!(value_of(&text, "neo_storage_read_cache_hits_total"), 100.0);
        assert_eq!(value_of(&text, "neo_storage_read_cache_misses_total"), 20.0);
        assert_eq!(
            value_of(&text, "neo_storage_read_cache_evictions_total"),
            7.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_read_cache_admissions_total"),
            90.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_storage_read_cache_rejected_total{reason=\"oversized\"}"
            ),
            5.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_storage_read_cache_rejected_total{reason=\"budget\"}"
            ),
            2.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_read_cache_prefetch_stale_skips_total"),
            6.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_read_cache_bloom_checks_total"),
            40.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_read_cache_bloom_negatives_total"),
            12.0
        );
        assert_eq!(value_of(&text, "neo_storage_read_cache_entries"), 42.0);
        assert_eq!(value_of(&text, "neo_storage_read_cache_bytes"), 4096.0);

        assert_eq!(
            value_of(&text, "neo_storage_write_buffer_flushes_total"),
            8.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_write_buffer_pending_operations"),
            12.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_write_buffer_pending_bytes"),
            2048.0
        );
        assert_eq!(
            value_of(&text, "neo_storage_write_buffer_bytes_written_total"),
            65536.0
        );

        assert_eq!(
            value_of(&text, "neo_state_root_node_cache_hits_total"),
            64.0
        );
        assert_eq!(
            value_of(&text, "neo_state_root_node_cache_misses_total"),
            16.0
        );
        assert_eq!(value_of(&text, "neo_state_root_warm_root_nodes"), 300.0);
        assert_eq!(value_of(&text, "neo_state_root_warm_root_bytes"), 98304.0);

        assert_eq!(
            value_of(&text, "neo_native_hook_seconds_total{phase=\"on_persist\"}"),
            2.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_native_hook_seconds_total{phase=\"post_persist\"}"
            ),
            0.5
        );
        assert_eq!(
            value_of(
                &text,
                "neo_native_hook_empty_block_seconds_total{phase=\"on_persist\"}"
            ),
            0.25
        );
        assert_eq!(
            value_of(
                &text,
                "neo_native_hook_empty_block_seconds_total{phase=\"post_persist\"}"
            ),
            0.05
        );
    }

    #[test]
    fn snapshot_converts_durations_to_seconds() {
        let snapshot = populated_snapshot();
        assert!((snapshot.mpt_stage_seconds(MptStage::Update) - 1.5).abs() < 1e-9);
        assert!((snapshot.mpt_stage_seconds(MptStage::Hash) - 0.25).abs() < 1e-9);
        assert!((snapshot.mpt_stage_seconds(MptStage::Hash256) - 0.0005).abs() < 1e-9);
        assert_eq!(snapshot.mpt_stage_seconds(MptStage::Commit), 0.0);
        assert!((snapshot.native_hook_seconds(NativeHookPhase::OnPersist) - 2.0).abs() < 1e-9);
        assert!((snapshot.native_hook_seconds(NativeHookPhase::PostPersist) - 0.5).abs() < 1e-9);
        assert!(
            (snapshot.native_hook_empty_block_seconds(NativeHookPhase::OnPersist) - 0.25).abs()
                < 1e-9
        );

        let text = snapshot.export_prometheus();
        assert!(
            (value_of(&text, "neo_mpt_stage_seconds_total{stage=\"update\"}") - 1.5).abs() < 1e-9
        );
        assert!(
            (value_of(&text, "neo_storage_write_buffer_flush_seconds_total") - 4.0).abs() < 1e-9
        );
        assert_eq!(value_of(&text, "neo_execution_profiling_enabled"), 1.0);
    }

    #[test]
    fn parallel_counters_export_every_family() {
        let parallel = ParallelExecutionSnapshot {
            speculative_blocks: 3,
            speculative_transactions: 11,
            conflicts: 4,
            reexecutions: 4,
            serial_fallbacks: 2,
            parallel_nanos: 1_500_000_000,
            serial_nanos: 2_000_000_000,
        };
        let snapshot = AccelerationSnapshot::capture(
            FastPathStatsSnapshot::default(),
            parallel,
            ExecutionMode::Auto,
            &ExecutionProfileSnapshot::default(),
            None,
            None,
        );
        let text = snapshot.export_prometheus();

        assert_eq!(
            value_of(&text, "neo_parallel_speculative_blocks_total"),
            3.0
        );
        assert_eq!(
            value_of(&text, "neo_parallel_speculative_transactions_total"),
            11.0
        );
        assert_eq!(value_of(&text, "neo_parallel_conflicts_total"), 4.0);
        assert_eq!(value_of(&text, "neo_parallel_reexecutions_total"), 4.0);
        assert_eq!(value_of(&text, "neo_parallel_serial_fallbacks_total"), 2.0);
        assert_eq!(
            value_of(&text, "neo_parallel_seconds_total{phase=\"speculative\"}"),
            1.5
        );
        assert_eq!(
            value_of(&text, "neo_parallel_seconds_total{phase=\"serial\"}"),
            2.0
        );
    }

    #[test]
    fn mode_family_is_one_hot() {
        let text = populated_snapshot().export_prometheus();
        let mut active = 0.0;
        for mode in ExecutionMode::ALL {
            let value = value_of(
                &text,
                &format!("neo_fast_path_mode{{mode=\"{}\"}}", mode.as_str()),
            );
            assert!(value == 0.0 || value == 1.0, "mode gauge is 0/1");
            active += value;
        }
        assert_eq!(active, 1.0);
        assert_eq!(
            value_of(&text, "neo_fast_path_mode{mode=\"auto\"}"),
            1.0,
            "auto is the configured mode"
        );
    }

    #[test]
    fn absent_components_report_zero_and_disabled() {
        let snapshot = AccelerationSnapshot::capture(
            FastPathStatsSnapshot::default(),
            ParallelExecutionSnapshot::default(),
            ExecutionMode::Canonical,
            &ExecutionProfileSnapshot::default(),
            None,
            None,
        );
        let text = snapshot.export_prometheus();

        assert_eq!(
            value_of(
                &text,
                "neo_storage_acceleration_enabled{component=\"read_cache\"}"
            ),
            0.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_storage_acceleration_enabled{component=\"write_buffer\"}"
            ),
            0.0
        );
        assert_eq!(value_of(&text, "neo_state_root_node_cache_enabled"), 0.0);
        assert_eq!(value_of(&text, "neo_execution_profiling_enabled"), 0.0);
        assert_eq!(value_of(&text, "neo_storage_read_cache_hits_total"), 0.0);
        assert_eq!(
            value_of(&text, "neo_storage_write_buffer_flushes_total"),
            0.0
        );
        assert_eq!(
            value_of(&text, "neo_mpt_stage_seconds_total{stage=\"hash\"}"),
            0.0
        );
        assert_eq!(
            value_of(&text, "neo_native_hook_seconds_total{phase=\"on_persist\"}"),
            0.0
        );
        assert_eq!(
            value_of(
                &text,
                "neo_native_hook_empty_block_seconds_total{phase=\"post_persist\"}"
            ),
            0.0
        );
    }

    #[test]
    fn present_components_report_enabled() {
        let text = populated_snapshot().export_prometheus();
        for component in ["read_cache", "write_buffer"] {
            assert_eq!(
                value_of(
                    &text,
                    &format!("neo_storage_acceleration_enabled{{component=\"{component}\"}}")
                ),
                1.0
            );
        }
        assert_eq!(value_of(&text, "neo_state_root_node_cache_enabled"), 1.0);
    }

    #[test]
    fn gather_prometheus_includes_acceleration_families() {
        let _guard = TEST_LOCK.lock();
        update_acceleration_metrics(populated_snapshot());
        let text = String::from_utf8(gather_prometheus()).expect("utf-8 exposition");

        assert!(text.contains("neo_fast_path_direct_attempts_total"));
        assert!(text.contains("neo_block_height"), "node families are kept");
        assert_eq!(acceleration_snapshot().fast_path.direct_attempts, 11);

        update_acceleration_metrics(AccelerationSnapshot::default());
        let cleared = String::from_utf8(gather_prometheus()).expect("utf-8 exposition");
        assert_eq!(
            value_of(&cleared, "neo_fast_path_direct_attempts_total"),
            0.0
        );
    }

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

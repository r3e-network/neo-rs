use std::sync::LazyLock;

use neo_storage::persistence::read_cache::ReadCacheStatsSnapshot;
use neo_storage::persistence::store::StoreAccelerationStats;
use neo_storage::persistence::write_batch_buffer::WriteBatchStatsSnapshot;
use neo_system::state_service::StateRootNodeCacheStats;
use neo_system::{
    ExecutionMode, ExecutionProfileSnapshot, FastPathStatsSnapshot, ParallelExecutionSnapshot,
};
use parking_lot::RwLock;

use super::enums::{
    FastPathOutcome, MPT_STAGE_COUNT, MptStage, NATIVE_HOOK_PHASE_COUNT, NativeHookPhase,
    RejectedInsert,
};

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

    pub(super) fn direct_outcome(&self, outcome: FastPathOutcome) -> u64 {
        match outcome {
            FastPathOutcome::Halt => self.fast_path.direct_halts,
            FastPathOutcome::Fault => self.fast_path.direct_faults,
        }
    }

    pub(super) fn rejected_insert(&self, reason: RejectedInsert) -> u64 {
        let Some(cache) = self.read_cache.as_ref() else {
            return 0;
        };
        match reason {
            RejectedInsert::Oversized => cache.rejected_oversized,
            RejectedInsert::Budget => cache.rejected_budget,
        }
    }

    pub(super) fn read_cache_or_default(&self) -> ReadCacheStatsSnapshot {
        self.read_cache.unwrap_or_default()
    }

    pub(super) fn write_buffer_or_default(&self) -> WriteBatchStatsSnapshot {
        self.write_buffer.unwrap_or_default()
    }

    pub(super) fn state_root_enabled(&self) -> bool {
        self.state_root_node_cache
            .is_some_and(|stats| stats.enabled)
    }
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

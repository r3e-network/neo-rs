use super::*;
#[derive(Debug, Default, Serialize)]
pub(super) struct ReplayProfile {
    pub(super) upstream_seconds: f64,
    pub(super) upstream_wait_seconds: f64,
    pub(super) decode_compare_seconds: f64,
    pub(super) local_execution_seconds: f64,
    pub(super) receipt_compare_seconds: f64,
    pub(super) durable_flush_seconds: f64,
    pub(super) audit_seconds: f64,
}

impl ReplayProfile {
    pub(super) fn measured_seconds(&self) -> f64 {
        self.upstream_seconds
            + self.decode_compare_seconds
            + self.local_execution_seconds
            + self.receipt_compare_seconds
            + self.durable_flush_seconds
            + self.audit_seconds
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ExecutionStageProfile {
    enabled: bool,
    blocks: u64,
    transaction_executions: u64,
    snapshot_open_seconds: f64,
    transaction_hash_seconds: f64,
    on_persist_seconds: f64,
    transaction_loop_seconds: f64,
    transaction_vm_seconds: f64,
    post_persist_seconds: f64,
    participant_precommit_seconds: f64,
    state_change_extraction_seconds: f64,
    mpt_seconds: f64,
    mpt_update_seconds: f64,
    mpt_commit_seconds: f64,
    mpt_root_hash_seconds: f64,
    mpt_hash_seconds: f64,
    mpt_serialization_seconds: f64,
    mpt_hash256_seconds: f64,
    mpt_node_cache_seconds: f64,
    mpt_node_read_seconds: f64,
    mpt_node_deserialize_seconds: f64,
    mpt_node_encode_seconds: f64,
    mpt_node_write_seconds: f64,
    store_commit_seconds: f64,
    store_writer_snapshot_open_seconds: f64,
    store_change_application_seconds: f64,
    store_write_batch_build_seconds: f64,
    store_database_write_seconds: f64,
    store_cache_invalidation_seconds: f64,
    runtime_update_seconds: f64,
    post_commit_seconds: f64,
    /// Aggregate stages restricted to zero-transaction blocks.
    empty_blocks: EmptyBlockStageProfile,
    /// Nested host wall-clock timings, keyed by Neo's four-byte syscall hash.
    syscalls: Vec<SyscallStageProfile>,
    /// Inclusive dynamic contract calls; nested calls overlap their callers.
    contract_calls: Vec<ContractCallStageProfile>,
    opcodes: Vec<OpcodeStageProfile>,
    external_instructions: u64,
}

#[derive(Debug, Serialize)]
struct EmptyBlockStageProfile {
    blocks: u64,
    snapshot_open_seconds: f64,
    on_persist_seconds: f64,
    transaction_loop_seconds: f64,
    post_persist_seconds: f64,
    participant_precommit_seconds: f64,
    state_change_extraction_seconds: f64,
    mpt_seconds: f64,
    mpt_update_seconds: f64,
    mpt_commit_seconds: f64,
    mpt_hash_seconds: f64,
    mpt_serialization_seconds: f64,
    mpt_hash256_seconds: f64,
    mpt_node_cache_seconds: f64,
    mpt_node_read_seconds: f64,
    mpt_node_deserialize_seconds: f64,
    mpt_node_encode_seconds: f64,
    mpt_node_write_seconds: f64,
    store_commit_seconds: f64,
    store_database_write_seconds: f64,
    runtime_update_seconds: f64,
    post_commit_seconds: f64,
}

#[derive(Debug, Serialize)]
struct OpcodeStageProfile {
    opcode: String,
    count: u64,
}

#[derive(Debug, Serialize)]
struct SyscallStageProfile {
    hash: String,
    calls: u64,
    elapsed_seconds: f64,
}

#[derive(Debug, Serialize)]
struct ContractCallStageProfile {
    contract_hash: String,
    method: String,
    argument_count: usize,
    calls: u64,
    elapsed_seconds: f64,
}

impl From<neo_system::ExecutionProfileSnapshot> for ExecutionStageProfile {
    fn from(profile: neo_system::ExecutionProfileSnapshot) -> Self {
        Self {
            enabled: profile.enabled,
            blocks: profile.blocks,
            transaction_executions: profile.transaction_executions,
            snapshot_open_seconds: profile.snapshot_open.as_secs_f64(),
            transaction_hash_seconds: profile.transaction_hash.as_secs_f64(),
            on_persist_seconds: profile.on_persist.as_secs_f64(),
            transaction_loop_seconds: profile.transaction_loop.as_secs_f64(),
            transaction_vm_seconds: profile.transaction_vm.as_secs_f64(),
            post_persist_seconds: profile.post_persist.as_secs_f64(),
            participant_precommit_seconds: profile.participant_precommit.as_secs_f64(),
            state_change_extraction_seconds: profile.state_change_extraction.as_secs_f64(),
            mpt_seconds: profile.mpt.as_secs_f64(),
            mpt_update_seconds: profile.mpt_update.as_secs_f64(),
            mpt_commit_seconds: profile.mpt_commit.as_secs_f64(),
            mpt_root_hash_seconds: profile.mpt_root_hash.as_secs_f64(),
            mpt_hash_seconds: profile.mpt_hash.as_secs_f64(),
            mpt_serialization_seconds: profile.mpt_serialization.as_secs_f64(),
            mpt_hash256_seconds: profile.mpt_hash256.as_secs_f64(),
            mpt_node_cache_seconds: profile.mpt_node_cache.as_secs_f64(),
            mpt_node_read_seconds: profile.mpt_node_read.as_secs_f64(),
            mpt_node_deserialize_seconds: profile.mpt_node_deserialize.as_secs_f64(),
            mpt_node_encode_seconds: profile.mpt_node_encode.as_secs_f64(),
            mpt_node_write_seconds: profile.mpt_node_write.as_secs_f64(),
            store_commit_seconds: profile.store_commit.as_secs_f64(),
            store_writer_snapshot_open_seconds: profile.store_writer_snapshot_open.as_secs_f64(),
            store_change_application_seconds: profile.store_change_application.as_secs_f64(),
            store_write_batch_build_seconds: profile.store_write_batch_build.as_secs_f64(),
            store_database_write_seconds: profile.store_database_write.as_secs_f64(),
            store_cache_invalidation_seconds: profile.store_cache_invalidation.as_secs_f64(),
            runtime_update_seconds: profile.runtime_update.as_secs_f64(),
            post_commit_seconds: profile.post_commit.as_secs_f64(),
            empty_blocks: EmptyBlockStageProfile {
                blocks: profile.empty_blocks.blocks,
                snapshot_open_seconds: profile.empty_blocks.snapshot_open.as_secs_f64(),
                on_persist_seconds: profile.empty_blocks.on_persist.as_secs_f64(),
                transaction_loop_seconds: profile.empty_blocks.transaction_loop.as_secs_f64(),
                post_persist_seconds: profile.empty_blocks.post_persist.as_secs_f64(),
                participant_precommit_seconds: profile
                    .empty_blocks
                    .participant_precommit
                    .as_secs_f64(),
                state_change_extraction_seconds: profile
                    .empty_blocks
                    .state_change_extraction
                    .as_secs_f64(),
                mpt_seconds: profile.empty_blocks.mpt.as_secs_f64(),
                mpt_update_seconds: profile.empty_blocks.mpt_update.as_secs_f64(),
                mpt_commit_seconds: profile.empty_blocks.mpt_commit.as_secs_f64(),
                mpt_hash_seconds: profile.empty_blocks.mpt_hash.as_secs_f64(),
                mpt_serialization_seconds: profile.empty_blocks.mpt_serialization.as_secs_f64(),
                mpt_hash256_seconds: profile.empty_blocks.mpt_hash256.as_secs_f64(),
                mpt_node_cache_seconds: profile.empty_blocks.mpt_node_cache.as_secs_f64(),
                mpt_node_read_seconds: profile.empty_blocks.mpt_node_read.as_secs_f64(),
                mpt_node_deserialize_seconds: profile
                    .empty_blocks
                    .mpt_node_deserialize
                    .as_secs_f64(),
                mpt_node_encode_seconds: profile.empty_blocks.mpt_node_encode.as_secs_f64(),
                mpt_node_write_seconds: profile.empty_blocks.mpt_node_write.as_secs_f64(),
                store_commit_seconds: profile.empty_blocks.store_commit.as_secs_f64(),
                store_database_write_seconds: profile
                    .empty_blocks
                    .store_database_write
                    .as_secs_f64(),
                runtime_update_seconds: profile.empty_blocks.runtime_update.as_secs_f64(),
                post_commit_seconds: profile.empty_blocks.post_commit.as_secs_f64(),
            },
            syscalls: profile
                .syscalls
                .into_iter()
                .map(|syscall| SyscallStageProfile {
                    hash: format!("0x{:08x}", syscall.hash),
                    calls: syscall.calls,
                    elapsed_seconds: syscall.elapsed.as_secs_f64(),
                })
                .collect(),
            contract_calls: profile
                .contract_calls
                .into_iter()
                .map(|call| ContractCallStageProfile {
                    contract_hash: call.contract_hash,
                    method: call.method,
                    argument_count: call.argument_count,
                    calls: call.calls,
                    elapsed_seconds: call.elapsed.as_secs_f64(),
                })
                .collect(),
            opcodes: profile
                .opcodes
                .into_iter()
                .map(|opcode| OpcodeStageProfile {
                    opcode: format!("0x{:02x}", opcode.opcode),
                    count: opcode.count,
                })
                .collect(),
            external_instructions: profile.external_instructions,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MptNodeCacheProfile {
    enabled: bool,
    capacity: usize,
    entries: usize,
    hits: u64,
    misses: u64,
    hit_rate_percent: f32,
    warm_root_nodes: usize,
    warm_root_estimated_bytes: usize,
}

impl From<neo_state_service::StateRootNodeCacheStats> for MptNodeCacheProfile {
    fn from(stats: neo_state_service::StateRootNodeCacheStats) -> Self {
        Self {
            enabled: stats.enabled,
            capacity: stats.capacity,
            entries: stats.entries,
            hits: stats.hits,
            misses: stats.misses,
            hit_rate_percent: stats.hit_rate_percent,
            warm_root_nodes: stats.warm_root_nodes,
            warm_root_estimated_bytes: stats.warm_root_estimated_bytes,
        }
    }
}

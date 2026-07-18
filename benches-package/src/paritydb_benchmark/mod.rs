//! # ParityDB persistence benchmark
//!
//! Benchmark-only adapter for a ParityDB hash-index column configured for
//! uniformly distributed Neo MPT node hashes. The `0xf0` namespace byte is
//! implicit in the column, so ParityDB receives the exact 32-byte node hash.
//!
//! This module is not linked into the node. The ordinary `Db::commit` call
//! only admits work to an asynchronous pipeline, so each durable benchmark
//! boundary closes the database, reopens it, and verifies a transaction
//! sentinel. ParityDB 0.5.5 exposes no cheaper supported commit receipt.
//!
//! ## Boundary
//!
//! This benchmark-only module owns ParityDB campaign configuration, execution,
//! and evidence reporting. Production node storage never depends on it.
//!
//! ## Contents
//!
//! - `runner`: campaign orchestration and report publication.
//! - `store`: ParityDB-specific durable operations and measurements.

mod runner;
mod store;

use crate::mdbx_benchmark::{
    BenchmarkLabels, CampaignScale, DigestReport, EvidenceDelta, EvidenceSnapshot, LogicalVolume,
    ReadBenchmarksReport, SampledRssPeak, SmokeSettings, WorkloadReport,
};
use crate::storage_workload::WorkloadShape;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;

pub use runner::run_paritydb_benchmark;

pub(crate) const PARITY_DB_CRATE_VERSION: &str = "0.5.5";

/// Complete input for one isolated ParityDB campaign.
#[derive(Debug, Clone)]
pub struct ParityDbBenchmarkConfig {
    /// Fresh ParityDB directory. Existing nonempty directories are rejected.
    pub database: PathBuf,
    /// Atomically published JSON report path.
    pub output: PathBuf,
    /// Optional JSONL evidence checkpoint path.
    pub evidence_log: Option<PathBuf>,
    /// Backend-neutral measured workload.
    pub shape: WorkloadShape,
    /// Exact or bounded campaign scale.
    pub scale: CampaignScale,
    /// Smoke-only bounds.
    pub smoke: SmokeSettings,
    /// Maximum operations in one durable prefill transaction.
    pub prefill_batch_entries: usize,
    /// Exact number of point queries per round.
    pub point_queries: usize,
    /// Point-query repetitions.
    pub point_rounds: u32,
    /// Maximum sorted keys in one measured lookup call.
    pub sorted_batch_keys: usize,
    /// Sorted-batch repetitions.
    pub sorted_batch_rounds: u32,
    /// Declared hardware, filesystem, durability, and cache state.
    pub labels: BenchmarkLabels,
}

/// Effective ParityDB column and durability configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParityDbConfigurationReport {
    /// Tested crate release.
    pub crate_version: String,
    /// Hash-index column id.
    pub column: u8,
    /// Stored key width after making the `0xf0` namespace implicit.
    pub stored_key_bytes: usize,
    /// Namespace byte represented by the dedicated column.
    pub implicit_namespace: u8,
    /// Whether ParityDB treats the first 32 key bytes as uniformly distributed.
    pub uniform: bool,
    /// Whether values are treated as key preimages.
    pub preimage: bool,
    /// Whether ParityDB owns a separate reference counter.
    pub ref_counted: bool,
    /// Column compression policy.
    pub compression: String,
    /// Whether a B-tree index is used.
    pub btree_index: bool,
    /// Whether WAL files are synced before enactment.
    pub sync_wal: bool,
    /// Whether enacted table mappings are synced before WAL recycling.
    pub sync_data: bool,
    /// Whether ordinary ParityDB background workers run in the benchmark.
    pub background_threads: bool,
    /// Exact fence mechanism used after every logical commit.
    pub durability_fence: String,
}

/// Timings collected from one supported ParityDB close/reopen fence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ParityDbStageTotals {
    /// User transactions admitted to the commit queue.
    pub durable_fences: u64,
    /// Time in `Db::commit` queue admission and overlay publication.
    pub commit_enqueue_ns: u64,
    /// Time closing the writer and draining queued user commits and WAL work.
    pub close_drain_ns: u64,
    /// Time reopening the writer after each close.
    pub reopen_ns: u64,
    /// Time verifying one exact put sentinel after each reopen.
    pub post_reopen_verify_ns: u64,
}

impl ParityDbStageTotals {
    pub(crate) fn merge(&mut self, other: Self) -> Result<()> {
        macro_rules! add {
            ($field:ident) => {
                self.$field = self
                    .$field
                    .checked_add(other.$field)
                    .context(concat!(stringify!($field), " overflows u64"))?;
            };
        }
        add!(durable_fences);
        add!(commit_enqueue_ns);
        add!(close_drain_ns);
        add!(reopen_ns);
        add!(post_reopen_verify_ns);
        Ok(())
    }
}

/// One measured workload phase with process, filesystem, and store evidence.
#[derive(Debug, Clone, Serialize)]
pub struct ParityDbPhaseReport {
    /// Stable phase name.
    pub name: String,
    /// Complete phase wall time.
    pub wall_ns: u64,
    /// Exact backend-neutral workload volume.
    pub logical: LogicalVolume,
    /// Boundary evidence before the phase.
    pub before: EvidenceSnapshot,
    /// Boundary evidence after the phase.
    pub after: EvidenceSnapshot,
    /// Process and retained-file changes.
    pub evidence_delta: EvidenceDelta,
    /// Bounded RSS sampler result.
    pub sampled_rss: SampledRssPeak,
    /// Process-attributed storage writes when procfs exposes them.
    pub process_physical_write_bytes: Option<u64>,
    /// Physical writes divided by exact logical value bytes.
    pub write_amplification_vs_values: Option<f64>,
    /// Physical writes divided by logical key-plus-value bytes.
    pub write_amplification_vs_mutations: Option<f64>,
    /// Backend stage timings and fence counts.
    pub stages: ParityDbStageTotals,
}

/// Throughput derived from the complete measured campaign phase.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ParityDbThroughputReport {
    /// Represented blocks per second.
    pub blocks_per_second: f64,
    /// Logical operations per second.
    pub operations_per_second: f64,
    /// Logical value MiB per second.
    pub value_mib_per_second: f64,
    /// Logical key-plus-value MiB per second.
    pub mutation_mib_per_second: f64,
}

/// Final bounded hash-index maintenance result.
#[derive(Debug, Clone, Serialize)]
pub struct ParityDbMaintenanceReport {
    /// Whether the crate exposes an ordinary compaction call.
    pub explicit_compaction_api: bool,
    /// Whether exact reindex cycles or time are exposed.
    pub reindex_metrics_available: bool,
    /// Hash-index generations present after final read-only reopen.
    pub hash_index_generations: u64,
    /// More than one hash index implies unfinished reindex/cleanup debt.
    pub pending_reindex_inferred: bool,
    /// Exact shutdown and outstanding-maintenance semantics.
    pub completion_semantics: String,
}

/// Reopen and exact sampled-value verification.
#[derive(Debug, Clone, Serialize)]
pub struct ParityDbReopenReport {
    /// Read-only open duration.
    pub open_ns: u64,
    /// Sampled present, tombstoned, and absent keys checked.
    pub verified_keys: u64,
    /// Expected verification digest.
    pub expected_digest: String,
    /// Digest read from the reopened database.
    pub actual_digest: String,
    /// Exact digest comparison outcome.
    pub matched: bool,
    /// Physical value entries reported after reopen, when the crate can derive
    /// them from all value-table free lists.
    pub value_entries: Option<u64>,
}

/// Final retained ParityDB layout.
#[derive(Debug, Clone, Serialize)]
pub struct ParityDbLayoutReport {
    /// Regular database files.
    pub regular_files: u64,
    /// Sum of logical file lengths.
    pub logical_bytes: u64,
    /// Retained allocated bytes when available.
    pub allocated_bytes: Option<u64>,
    /// Live physical value entries when ParityDB can derive the count.
    pub value_entries: Option<u64>,
    /// Sorted `index_00_*` generation filenames retained after reopen.
    pub hash_index_files: Vec<String>,
}

/// Machine-readable result of one ParityDB candidate campaign.
#[derive(Debug, Clone, Serialize)]
pub struct ParityDbBenchmarkReport {
    /// Report schema version.
    pub schema_version: u32,
    /// Terminal campaign status.
    pub status: String,
    /// Exact backend label.
    pub backend: String,
    /// Published report path.
    pub output: PathBuf,
    /// Measured database path.
    pub database: PathBuf,
    /// Optional evidence checkpoint path.
    pub evidence_log: Option<PathBuf>,
    /// Exact or smoke campaign scale.
    pub scale: CampaignScale,
    /// Declared benchmark environment.
    pub labels: BenchmarkLabels,
    /// Fully resolved backend-neutral workload.
    pub workload: WorkloadReport,
    /// Effective ParityDB configuration.
    pub configuration: ParityDbConfigurationReport,
    /// Durable prefill evidence.
    pub prefill: ParityDbPhaseReport,
    /// Timed campaign evidence.
    pub campaign: ParityDbPhaseReport,
    /// Throughput from the timed campaign wall.
    pub campaign_throughput: ParityDbThroughputReport,
    /// Point and sorted-loop batch lookup evidence.
    pub reads: ReadBenchmarksReport,
    /// Derived-index maintenance evidence.
    pub maintenance: ParityDbMaintenanceReport,
    /// Read-only reopen verification.
    pub reopen: ParityDbReopenReport,
    /// Final physical layout.
    pub layout: ParityDbLayoutReport,
    /// Backend-neutral operation-stream digests.
    pub digests: DigestReport,
    /// Whole runner wall time.
    pub total_wall_ns: u64,
    /// Explicit interpretation and durability limits.
    pub evidence_limitations: Vec<String>,
}

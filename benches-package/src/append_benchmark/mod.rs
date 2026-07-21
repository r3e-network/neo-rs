//! # Append-pack benchmark
//!
//! Benchmark-only append frames with immutable sorted index runs.
//!
//! Reads probe per-run xor16 membership filters and a sparse fence index
//! newest-first; index records stay on disk behind positioned reads.
//! Leveled compaction merges the derived runs (payload frames are never
//! rewritten), immutable manifest generations gate visibility atomically,
//! and snapshot leases pin generations until explicit reclamation.
//!
//! The engine itself was extracted into the `neo-state-packs` workspace
//! crate; this module keeps the benchmark runner and reporting vocabulary.
//!
//! ## Boundary
//!
//! This module owns campaign configuration and evidence reporting. Pack format,
//! recovery, and lookup semantics remain in `neo-state-packs`.
//!
//! ## Contents
//!
//! - Append campaign runner.
//! - Benchmark-only compatibility store helpers.
//! - Stable phase, percentile, and report schemas.

mod runner;
mod store;

use crate::mdbx_benchmark::{
    BenchmarkLabels, CampaignScale, EvidenceDelta, EvidenceSnapshot, LogicalVolume, SampledRssPeak,
    SmokeSettings, WorkloadReport,
};
use crate::storage_workload::WorkloadShape;
use serde::Serialize;
use std::path::PathBuf;

pub use runner::run_append_benchmark;

pub(crate) use neo_state_packs::PackStageTotals as AppendStageTotals;

/// Complete input for one isolated append prototype campaign.
#[derive(Debug, Clone)]
pub struct AppendBenchmarkConfig {
    /// Fresh prototype database directory.
    pub database: PathBuf,
    /// Atomically published JSON report path.
    pub output: PathBuf,
    /// Optional JSONL evidence checkpoint path.
    pub evidence_log: Option<PathBuf>,
    /// Backend-neutral measured workload.
    pub shape: WorkloadShape,
    /// Exact or bounded campaign scale.
    pub scale: CampaignScale,
    /// Bounds used for a smoke projection.
    pub smoke: SmokeSettings,
    /// Maximum operations in one durable prefill frame.
    pub prefill_batch_entries: usize,
    /// Exact number of point-read queries per round.
    pub point_queries: usize,
    /// Point-read repetitions.
    pub point_rounds: u32,
    /// Maximum keys in one sorted batch lookup.
    pub sorted_batch_keys: usize,
    /// Sorted-batch repetitions.
    pub sorted_batch_rounds: u32,
    /// Hard bound for decoded immutable index entries.
    pub max_index_memory_bytes: u64,
    /// Declared hardware, filesystem, durability, and cache state.
    pub labels: BenchmarkLabels,
}

/// Stable percentile summary using nearest-rank selection.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PercentileReport {
    /// Number of retained latency samples.
    pub samples: u64,
    /// Minimum latency.
    pub min_ns: u64,
    /// Arithmetic mean latency.
    pub mean_ns: u64,
    /// Nearest-rank median latency.
    pub p50_ns: u64,
    /// Nearest-rank 95th percentile latency.
    pub p95_ns: u64,
    /// Nearest-rank 99th percentile latency.
    pub p99_ns: u64,
    /// Maximum latency.
    pub max_ns: u64,
}

/// Timed phase with process, filesystem, and backend stage evidence.
#[derive(Debug, Clone, Serialize)]
pub struct AppendPhaseReport {
    /// Stable phase name.
    pub name: String,
    /// Complete measured phase wall time.
    pub wall_ns: u64,
    /// Logical workload presented to the store.
    pub logical: LogicalVolume,
    /// Process and file-tree evidence before the phase.
    pub before: EvidenceSnapshot,
    /// Process and file-tree evidence after the phase.
    pub after: EvidenceSnapshot,
    /// Signed process and file-tree changes.
    pub evidence_delta: EvidenceDelta,
    /// Bounded RSS sampler result.
    pub sampled_rss: SampledRssPeak,
    /// Process-attributed physical write bytes when available.
    pub process_physical_write_bytes: Option<u64>,
    /// Physical writes divided by logical value bytes.
    pub write_amplification_vs_values: Option<f64>,
    /// Physical writes divided by logical key-plus-value bytes.
    pub write_amplification_vs_mutations: Option<f64>,
    /// Time spent writing append-frame bytes.
    pub append_write_ns: u64,
    /// Time spent syncing the append pack.
    pub pack_sync_ns: u64,
    /// Time spent building immutable run bytes in memory.
    pub index_build_ns: u64,
    /// Wall time covering the overlapping pack sync and index build.
    pub publication_overlap_ns: u64,
    /// Time spent writing immutable index runs.
    pub index_write_ns: u64,
    /// Time spent syncing immutable index runs.
    pub index_sync_ns: u64,
    /// Time spent syncing the index-run directory.
    pub directory_sync_ns: u64,
    /// Durable frames published in this phase.
    pub frames: u64,
    /// Index records published in this phase.
    pub index_entries: u64,
}

/// Throughput derived from the exact measured campaign phase.
#[derive(Debug, Clone, Serialize)]
pub struct AppendThroughputReport {
    /// Represented blocks per measured second.
    pub blocks_per_second: f64,
    /// Mutations per measured second.
    pub operations_per_second: f64,
    /// Logical value MiB per measured second.
    pub value_mib_per_second: f64,
    /// Logical key-plus-value MiB per measured second.
    pub mutation_mib_per_second: f64,
}

/// Point or ordered-batch read evidence.
#[derive(Debug, Clone, Serialize)]
pub struct AppendReadReport {
    /// Point or sorted-batch read mode.
    pub mode: String,
    /// Declared cache state.
    pub cache_state: String,
    /// Number of corpus repetitions.
    pub rounds: u32,
    /// Total keys queried across repetitions.
    pub keys: u64,
    /// Total present-key results.
    pub hits: u64,
    /// Total absent-key results.
    pub misses: u64,
    /// Returned value bytes.
    pub value_bytes: u64,
    /// Requested keys per backend call.
    pub target_keys_per_call: usize,
    /// Per-call latency distribution.
    pub call_latency: PercentileReport,
    /// Latency normalized by keys in each call.
    pub normalized_per_key_latency: PercentileReport,
}

/// Point and sorted-batch read results for one store state.
#[derive(Debug, Clone, Serialize)]
pub struct AppendReadsReport {
    /// Stable percentile selection algorithm.
    pub percentile_method: String,
    /// Individual point-query results.
    pub point: AppendReadReport,
    /// Sorted multi-key query results.
    pub sorted_batch: AppendReadReport,
}

/// Reopen and exact sampled-value verification.
#[derive(Debug, Clone, Serialize)]
pub struct AppendReopenReport {
    /// Time to reopen and validate the prototype.
    pub open_ns: u64,
    /// Sampled keys checked after reopen.
    pub verified_keys: u64,
    /// Digest computed from expected sampled values.
    pub expected_digest: String,
    /// Digest computed from reopened sampled values.
    pub actual_digest: String,
    /// Whether expected and actual digests match.
    pub matched: bool,
    /// Append frames fully validated while opening.
    pub frames_validated: u64,
    /// Immutable index runs fully validated while opening.
    pub runs_validated: u64,
    /// Decoded index records validated while opening.
    pub index_entries: u64,
}

/// Physical prototype layout after the campaign.
#[derive(Debug, Clone, Serialize)]
pub struct AppendLayoutReport {
    /// Final append-pack length.
    pub pack_bytes: u64,
    /// Final immutable-index bytes.
    pub index_bytes: u64,
    /// Final immutable index-run count.
    pub run_count: u64,
    /// Estimated bytes retained by decoded index entries.
    pub decoded_index_memory_bytes: u64,
    /// Configured decoded-index memory bound.
    pub max_index_memory_bytes: u64,
}

/// Leveled compaction and reclamation evidence for one campaign.
#[derive(Debug, Clone, Serialize)]
pub struct AppendCompactionReport {
    /// Completed compaction cycles (one merge plus manifest publication each).
    pub cycles: u64,
    /// Input runs consumed across all cycles.
    pub runs_merged: u64,
    /// Output runs produced across all cycles.
    pub runs_produced: u64,
    /// Index records read from compaction inputs.
    pub input_records: u64,
    /// Index records written after newest-epoch-wins dedup.
    pub output_records: u64,
    /// Bytes of compacted run files written.
    pub bytes_written: u64,
    /// Wall time spent compacting inside the append path.
    pub wall_ns: u64,
    /// High-water mark of concurrently live runs.
    pub peak_live_runs: u64,
    /// Live runs in the final manifest generation.
    pub live_runs_final: u64,
    /// Explicit reclamation passes executed.
    pub gc_cycles: u64,
    /// Superseded run files deleted by reclamation.
    pub gc_runs_deleted: u64,
    /// Superseded manifests deleted by reclamation.
    pub gc_manifests_deleted: u64,
    /// Bytes reclaimed by deletion.
    pub gc_bytes_reclaimed: u64,
}

/// Machine-readable append prototype result.
#[derive(Debug, Clone, Serialize)]
pub struct AppendBenchmarkReport {
    /// Machine-readable schema version.
    pub schema_version: u32,
    /// Terminal campaign status.
    pub status: String,
    /// Exact prototype implementation label.
    pub backend: String,
    /// Published report path.
    pub output: PathBuf,
    /// Measured prototype directory.
    pub database: PathBuf,
    /// Optional checkpoint log path.
    pub evidence_log: Option<PathBuf>,
    /// Exact or smoke campaign scale.
    pub scale: CampaignScale,
    /// Declared benchmark environment.
    pub labels: BenchmarkLabels,
    /// Fully resolved workload parameters.
    pub workload: WorkloadReport,
    /// Durable prefill evidence.
    pub prefill: AppendPhaseReport,
    /// Timed campaign evidence.
    pub campaign: AppendPhaseReport,
    /// Throughput derived from the campaign wall time.
    pub campaign_throughput: AppendThroughputReport,
    /// Point and sorted-batch read evidence.
    pub reads: AppendReadsReport,
    /// Leveled compaction and reclamation evidence.
    pub compaction: AppendCompactionReport,
    /// Reopen validation evidence.
    pub reopen: AppendReopenReport,
    /// Final physical and decoded-index layout.
    pub layout: AppendLayoutReport,
    /// Backend-neutral prefill operation digest.
    pub prefill_sha256: String,
    /// Backend-neutral campaign operation digest.
    pub campaign_sha256: String,
    /// Complete runner wall time.
    pub total_wall_ns: u64,
    /// Explicit interpretation limits for this prototype.
    pub evidence_limitations: Vec<String>,
}

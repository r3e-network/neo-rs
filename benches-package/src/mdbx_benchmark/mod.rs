//! # MDBX persistence benchmark
//!
//! Durable MDBX persistence campaign runner and report schema.
//!
//! ## Boundary
//!
//! This module measures the production MDBX backend through declared workloads
//! and durability settings. It does not alter node storage policy.
//!
//! ## Contents
//!
//! - Process, filesystem, and RSS evidence capture.
//! - Durable workload runner.
//! - Stable campaign configuration and report schemas.

mod evidence;
mod runner;

use crate::storage_workload::{VALUE_SIZE_BUCKET_UPPER_BOUNDS, ValueSizeClass, WorkloadShape};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub use evidence::{
    EvidenceDelta, EvidenceLog, EvidenceSnapshot, FileTreeDelta, FileTreeSnapshot, ProcessDelta,
    ProcessIoSnapshot, ProcessSnapshot, RssSampler, SampledRssPeak, capture_evidence,
    clock_ticks_per_second, evidence_delta,
};
pub use runner::run_mdbx_benchmark;

/// Whether to execute the exact sustained corpus or a ratio-preserving smoke
/// projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CampaignScale {
    /// Exact measured prefill and operation stream.
    Full,
    /// Bounded projection for development and CI validation.
    Smoke,
}

impl CampaignScale {
    /// Stable report label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Smoke => "smoke",
        }
    }
}

/// Explicit bounds used only for the smoke projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SmokeSettings {
    /// Prefilled rows retained from the full corpus.
    pub prefill_rows: u64,
    /// Timed mutations retained from the full corpus.
    pub operations: u64,
    /// Logical blocks represented by the projected campaign.
    pub blocks: u64,
}

impl Default for SmokeSettings {
    fn default() -> Self {
        Self {
            prefill_rows: 32_768,
            operations: 8_192,
            blocks: 100,
        }
    }
}

/// Operator-provided benchmark identity required in every JSON report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BenchmarkLabels {
    /// Named machine or hardware profile.
    pub hardware: String,
    /// Named filesystem/device profile.
    pub filesystem: String,
    /// Explicit durability policy.
    pub durability: String,
    /// Cache-state declaration for read percentiles.
    pub read_cache_state: String,
}

/// Complete runner input.
#[derive(Debug, Clone)]
pub struct MdbxBenchmarkConfig {
    /// Fresh database directory. A nonempty directory is rejected.
    pub database: PathBuf,
    /// Optional JSONL phase checkpoint stream for external samplers.
    pub evidence_log: Option<PathBuf>,
    /// Backend-neutral measured shape.
    pub shape: WorkloadShape,
    /// Exact or projected campaign.
    pub scale: CampaignScale,
    /// Smoke-only bounds.
    pub smoke: SmokeSettings,
    /// Maximum entries materialized in each prefill transaction.
    pub prefill_batch_entries: usize,
    /// Point-query corpus size, split between present and absent keys.
    pub point_queries: usize,
    /// Point-query repetitions.
    pub point_rounds: u32,
    /// Keys per sorted MDBX batch read.
    pub sorted_batch_keys: usize,
    /// Sorted-batch corpus repetitions.
    pub sorted_batch_rounds: u32,
    /// Report identity.
    pub labels: BenchmarkLabels,
}

impl MdbxBenchmarkConfig {
    pub(crate) fn resolve(&self) -> Result<ResolvedBenchmarkConfig> {
        validate_nonempty_label("hardware", &self.labels.hardware)?;
        validate_nonempty_label("filesystem", &self.labels.filesystem)?;
        validate_nonempty_label("durability", &self.labels.durability)?;
        validate_nonempty_label("read cache state", &self.labels.read_cache_state)?;
        if self.prefill_batch_entries == 0 {
            bail!("prefill batch entries must be greater than zero");
        }
        if self.point_queries < 2 {
            bail!("point query count must be at least two");
        }
        if self.point_rounds == 0 || self.sorted_batch_rounds == 0 {
            bail!("read benchmark rounds must be greater than zero");
        }
        if self.sorted_batch_keys == 0 {
            bail!("sorted batch size must be greater than zero");
        }
        if self.scale == CampaignScale::Full && cfg!(debug_assertions) && !cfg!(test) {
            bail!(
                "full MDBX campaigns require a build with debug assertions disabled; rerun with --release"
            );
        }
        let effective_mdbx_sync_mode = require_durable_mdbx_sync_mode()?;
        let shape = resolve_shape(self.shape, self.scale, self.smoke)?;
        let required_hits = self.point_queries.div_ceil(2);
        let required_hits_u64 = u64::try_from(required_hits)
            .context("required prefill verification count does not fit u64")?;
        if required_hits_u64 > shape.prefill_rows {
            bail!(
                "point query corpus requires {required_hits} prefill rows but the resolved prefill has {}",
                shape.prefill_rows
            );
        }
        let version_hit_puts = shape
            .version_hit_count
            .checked_sub(shape.tombstone_count)
            .context("resolved version-hit put count underflows")?;
        let new_campaign_puts = shape
            .put_count
            .checked_sub(version_hit_puts)
            .context("resolved new campaign put count underflows")?;
        if required_hits_u64 > new_campaign_puts {
            bail!(
                "point query corpus requires {required_hits} newly inserted campaign rows but the resolved campaign has {new_campaign_puts}"
            );
        }
        Ok(ResolvedBenchmarkConfig {
            database: self.database.clone(),
            evidence_log: self.evidence_log.clone(),
            shape,
            scale: self.scale,
            smoke: self.smoke,
            prefill_batch_entries: self.prefill_batch_entries,
            point_queries: self.point_queries,
            point_rounds: self.point_rounds,
            sorted_batch_keys: self.sorted_batch_keys,
            sorted_batch_rounds: self.sorted_batch_rounds,
            labels: self.labels.clone(),
            effective_mdbx_sync_mode,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBenchmarkConfig {
    pub database: PathBuf,
    pub evidence_log: Option<PathBuf>,
    pub shape: WorkloadShape,
    pub scale: CampaignScale,
    pub smoke: SmokeSettings,
    pub prefill_batch_entries: usize,
    pub point_queries: usize,
    pub point_rounds: u32,
    pub sorted_batch_keys: usize,
    pub sorted_batch_rounds: u32,
    pub labels: BenchmarkLabels,
    pub effective_mdbx_sync_mode: String,
}

/// Exact logical data volume presented to a backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LogicalVolume {
    /// Put operations.
    pub puts: u64,
    /// Tombstone operations.
    pub tombstones: u64,
    /// Put plus tombstone operations.
    pub entries: u64,
    /// Key bytes across all operations.
    pub key_bytes: u64,
    /// Value bytes across puts.
    pub value_bytes: u64,
    /// Key plus value bytes supplied to the backend.
    pub mutation_bytes: u64,
    /// Put counts in the eight authoritative value-size buckets.
    pub value_size_counts: [u64; 8],
}

impl LogicalVolume {
    pub(crate) fn merge(&mut self, other: Self) -> Result<()> {
        let mut value_size_counts = [0u64; 8];
        for (index, value) in value_size_counts.iter_mut().enumerate() {
            *value = self.value_size_counts[index]
                .checked_add(other.value_size_counts[index])
                .context("logical value-size bucket count overflows")?;
        }
        *self = Self {
            puts: self
                .puts
                .checked_add(other.puts)
                .context("put count overflows")?,
            tombstones: self
                .tombstones
                .checked_add(other.tombstones)
                .context("tombstone count overflows")?,
            entries: self
                .entries
                .checked_add(other.entries)
                .context("entry count overflows")?,
            key_bytes: self
                .key_bytes
                .checked_add(other.key_bytes)
                .context("logical key byte count overflows")?,
            value_bytes: self
                .value_bytes
                .checked_add(other.value_bytes)
                .context("logical value byte count overflows")?,
            mutation_bytes: self
                .mutation_bytes
                .checked_add(other.mutation_bytes)
                .context("logical mutation byte count overflows")?,
            value_size_counts,
        };
        Ok(())
    }
}

/// Delta for one stable MDBX metric series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetricDelta {
    /// Stable stage/count label.
    pub name: String,
    /// Observation delta.
    pub samples: u64,
    /// Total delta.
    pub total: u64,
}

/// MDBX commit metrics isolated to one phase or epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MdbxMetricsDelta {
    /// Commit attempts.
    pub attempts: u64,
    /// Failed attempts.
    pub failures: u64,
    /// Durable committed transactions.
    pub committed_transactions: u64,
    /// Time spent inside durable MDBX transaction commits, in microseconds.
    pub durable_commit_us: u64,
    /// Timing stages in microseconds.
    pub stages: Vec<MetricDelta>,
    /// Logical entry/byte counters.
    pub counts: Vec<MetricDelta>,
}

/// One measured phase with process, retained-file, and MDBX evidence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseReport {
    /// Stable phase name.
    pub name: String,
    /// Wall duration.
    pub wall_ns: u64,
    /// Exact backend input volume.
    pub logical: LogicalVolume,
    /// Boundary evidence before work.
    pub before: EvidenceSnapshot,
    /// Boundary evidence after work.
    pub after: EvidenceSnapshot,
    /// Counter/footprint deltas.
    pub evidence_delta: EvidenceDelta,
    /// Bounded sampled RSS peak.
    pub sampled_rss: SampledRssPeak,
    /// MDBX-owned commit metric deltas.
    pub mdbx: MdbxMetricsDelta,
    /// Process-attributed storage-layer writes from `/proc/self/io`.
    pub process_physical_write_bytes: Option<u64>,
    /// Process write amplification relative to value bytes.
    pub write_amplification_vs_values: Option<f64>,
    /// Process write amplification relative to all mutation bytes.
    pub write_amplification_vs_mutations: Option<f64>,
}

/// Rates derived from one explicitly named campaign time denominator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ThroughputRates {
    /// Time denominator used for these rates.
    pub elapsed_ns: u64,
    /// Source blocks represented per wall-clock second.
    pub blocks_per_second: Option<f64>,
    /// Backend mutations committed per wall-clock second.
    pub operations_per_second: Option<f64>,
    /// Put-value MiB committed per wall-clock second.
    pub value_mib_per_second: Option<f64>,
    /// Key-plus-value mutation MiB committed per wall-clock second.
    pub mutation_mib_per_second: Option<f64>,
}

/// Separate end-to-end ingestion, backend, and durable-fence throughput.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ThroughputReport {
    /// Generation, accounting, digesting, backend commit, and verification wall.
    pub ingestion: ThroughputRates,
    /// MDBX raw-overlay commit path measured by its `total` timing stage.
    pub backend_commit: ThroughputRates,
    /// MDBX transaction commit/sync stage only.
    pub durable_commit: ThroughputRates,
}

/// One exact measured durable commit epoch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EpochReport {
    /// Zero-based epoch index.
    pub index: u32,
    /// Source blocks represented by the epoch.
    pub blocks: u64,
    /// Epoch measurement.
    pub phase: PhaseReport,
}

/// Nearest-rank latency distribution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LatencyPercentiles {
    /// Number of timed observations.
    pub samples: u64,
    /// Minimum nanoseconds.
    pub min_ns: u64,
    /// Arithmetic mean nanoseconds.
    pub mean_ns: u64,
    /// Median nanoseconds.
    pub p50_ns: u64,
    /// 95th percentile nanoseconds.
    pub p95_ns: u64,
    /// 99th percentile nanoseconds.
    pub p99_ns: u64,
    /// Maximum nanoseconds.
    pub max_ns: u64,
}

/// Verified latency results for one read mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadLatencyReport {
    /// Stable mode label.
    pub mode: String,
    /// Cache-state declaration supplied by the operator.
    pub cache_state: String,
    /// Repeated rounds.
    pub rounds: u32,
    /// Keys queried across every round.
    pub keys: u64,
    /// Expected present results.
    pub hits: u64,
    /// Expected absent results.
    pub misses: u64,
    /// Returned value bytes.
    pub value_bytes: u64,
    /// Configured upper bound for keys in one backend call.
    pub target_keys_per_call: usize,
    /// Minimum actual keys in a timed backend call.
    pub min_keys_per_call: usize,
    /// Maximum actual keys in a timed backend call.
    pub max_keys_per_call: usize,
    /// Whole-call latency distribution.
    pub call_latency: LatencyPercentiles,
    /// Whole-call latency divided by keys in that call.
    pub normalized_per_key_latency: LatencyPercentiles,
}

/// Point and sorted-batch lookup evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadBenchmarksReport {
    /// Percentile definition applied to every latency distribution.
    pub percentile_method: String,
    /// Individually timed point lookups.
    pub point: ReadLatencyReport,
    /// Sorted batch calls.
    pub sorted_batch: ReadLatencyReport,
}

/// Stratified reopen samples retained from one timed campaign epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ReopenEpochCoverageReport {
    /// Zero-based durable epoch index.
    pub epoch: u32,
    /// Newly inserted put keys sampled from this epoch.
    pub new_puts: u64,
    /// Existing-version put keys sampled from this epoch.
    pub version_hit_puts: u64,
    /// Tombstoned keys sampled from this epoch.
    pub tombstones: u64,
}

/// Read-only reopen verification result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReopenReport {
    /// Time to reopen the durable environment.
    pub open_ns: u64,
    /// Keys compared after reopen.
    pub verified_keys: u64,
    /// Verified keys originating in the prefilled working set.
    pub prefill_keys: u64,
    /// Verified keys first inserted by the timed campaign.
    pub campaign_keys: u64,
    /// Sampled existing-version puts across campaign epochs.
    pub version_hit_puts: u64,
    /// Sampled tombstones across campaign epochs.
    pub tombstones: u64,
    /// Synthetic absent keys verified after reopen.
    pub absent_keys: u64,
    /// Per-epoch mutation-class coverage. Counts may overlap the origin counts.
    pub epoch_coverage: Vec<ReopenEpochCoverageReport>,
    /// Expected verification-corpus SHA-256.
    pub expected_digest: String,
    /// Actual reopened verification-corpus SHA-256.
    pub actual_digest: String,
    /// Exact comparison outcome.
    pub matched: bool,
    /// MDBX transaction id immediately before closing the writer.
    pub pre_drop_transaction_id: u64,
    /// MDBX transaction id after reopen.
    pub transaction_id: u64,
}

/// Deterministic stream digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DigestReport {
    /// Prefill operation stream digest.
    pub prefill_sha256: String,
    /// Timed campaign operation stream digest.
    pub campaign_sha256: String,
}

/// Named, resolved workload parameters included in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkloadReport {
    /// Fixture/corpus source.
    pub source: String,
    /// Generator seed.
    pub seed: u64,
    /// Resolved prefill rows.
    pub prefill_rows: u64,
    /// Resolved blocks.
    pub blocks: u64,
    /// Resolved durable commits.
    pub commits: u32,
    /// Resolved puts.
    pub puts: u64,
    /// Resolved tombstones.
    pub tombstones: u64,
    /// Resolved existing-version operations.
    pub version_hits: u64,
    /// Per-bucket put counts.
    pub value_size_counts: [u64; 8],
    /// Per-bucket exact logical value bytes.
    pub value_size_bytes: [u64; 8],
}

impl From<WorkloadShape> for WorkloadReport {
    fn from(shape: WorkloadShape) -> Self {
        Self {
            source: shape.source.to_owned(),
            seed: shape.seed,
            prefill_rows: shape.prefill_rows,
            blocks: shape.blocks,
            commits: shape.commit_count,
            puts: shape.put_count,
            tombstones: shape.tombstone_count,
            version_hits: shape.version_hit_count,
            value_size_counts: shape.value_sizes.map(|value| value.put_count),
            value_size_bytes: shape.value_sizes.map(|value| value.total_bytes),
        }
    }
}

/// Resolved, serialized runner controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerControlsReport {
    /// Prefill entries per durable transaction.
    pub prefill_batch_entries: usize,
    /// Point-query corpus size.
    pub point_queries: usize,
    /// Point repetitions.
    pub point_rounds: u32,
    /// Sorted keys per call.
    pub sorted_batch_keys: usize,
    /// Sorted-batch repetitions.
    pub sorted_batch_rounds: u32,
    /// Smoke settings, retained even for a full report for auditability.
    pub smoke: SmokeSettings,
}

/// Build and source provenance for one benchmark executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BenchmarkBuildReport {
    /// Cargo package version.
    pub package_version: String,
    /// Cargo profile captured by the build script.
    pub profile: String,
    /// Rust optimization level captured by the build script.
    pub opt_level: String,
    /// Whether Rust debug assertions are enabled.
    pub debug_assertions: bool,
    /// Compilation target triple.
    pub target: String,
    /// Compilation host triple.
    pub host: String,
    /// Compiler version used by the build.
    pub rustc: String,
    /// Runtime executable path.
    pub executable: PathBuf,
    /// Git revision containing the benchmark source, when available.
    pub git_revision: Option<String>,
    /// Whether tracked or untracked worktree changes were present.
    pub git_dirty: Option<bool>,
}

/// Effective MDBX and raw-read tuning used by the campaign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MdbxRuntimeConfigReport {
    /// Effective transaction sync mode.
    pub sync_mode: String,
    /// Effective cursor write algorithm.
    pub cursor_write_mode: String,
    /// MDBX page coalescing flag.
    pub coalesce: bool,
    /// MDBX no-memory-initialization flag.
    pub no_meminit: bool,
    /// Optional prefix-occupancy sidecar path.
    pub prefix_index_path: Option<PathBuf>,
    /// Effective ordinary sorted-batch read parallelism.
    pub batch_read_threads: usize,
    /// Effective write-intent sorted-read parallelism.
    pub write_intent_read_threads: usize,
    /// Requested MDBX maximum geometry in bytes.
    pub geometry_upper_bytes: u64,
    /// Requested MDBX geometry growth step in bytes.
    pub geometry_growth_bytes: u64,
    /// Requested maximum MDBX reader slots.
    pub requested_max_readers: u32,
}

/// MDBX environment state after the durable campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MdbxEnvironmentReport {
    /// Current memory-map size.
    pub map_size: u64,
    /// Last used page number.
    pub last_page_number: u64,
    /// Last committed transaction id.
    pub transaction_id: u64,
    /// Reader capacity.
    pub max_readers: u64,
    /// Reader slots in use.
    pub readers: u64,
}

/// Versioned durable current-MDBX benchmark report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MdbxBenchmarkReport {
    /// Report schema version.
    pub schema_version: u32,
    /// Completion status.
    pub status: String,
    /// Backend under test.
    pub backend: String,
    /// OS process id used for external correlation.
    pub pid: u32,
    /// Unix start timestamp in nanoseconds.
    pub started_unix_ns: u128,
    /// Unix finish timestamp in nanoseconds.
    pub finished_unix_ns: u128,
    /// Whole runner duration.
    pub total_wall_ns: u64,
    /// Linux CPU clock frequency when available.
    pub clock_ticks_per_second: Option<u64>,
    /// Effective MDBX transaction sync mode, validated before opening the store.
    pub effective_mdbx_sync_mode: String,
    /// Exact or smoke mode.
    pub scale: CampaignScale,
    /// Operator identity labels.
    pub labels: BenchmarkLabels,
    /// Build and repository provenance.
    pub build: BenchmarkBuildReport,
    /// Effective MDBX tuning.
    pub mdbx_runtime: MdbxRuntimeConfigReport,
    /// Database path.
    pub database: PathBuf,
    /// Optional JSONL evidence path.
    pub evidence_log: Option<PathBuf>,
    /// Resolved workload.
    pub workload: WorkloadReport,
    /// Resolved controls.
    pub controls: RunnerControlsReport,
    /// Prefill measurement, excluded from timed campaign throughput.
    pub prefill: PhaseReport,
    /// Each exact durable campaign epoch.
    pub epochs: Vec<EpochReport>,
    /// Aggregate timed campaign measurement.
    pub campaign: PhaseReport,
    /// Derived throughput for the aggregate timed campaign.
    pub campaign_throughput: ThroughputReport,
    /// Final explicit MDBX durable fence after all epochs.
    pub final_flush_ns: u64,
    /// Point and sorted-batch read latency percentiles.
    pub reads: ReadBenchmarksReport,
    /// Read-only reopen verification.
    pub reopen: ReopenReport,
    /// Deterministic operation-stream digests.
    pub digests: DigestReport,
    /// MDBX environment state.
    pub environment: MdbxEnvironmentReport,
    /// Explicit measurement limitations.
    pub evidence_limitations: Vec<String>,
}

/// Writes a report through a synced temporary file and atomic rename.
pub fn write_json_report(path: &Path, report: &MdbxBenchmarkReport) -> Result<()> {
    validate_benchmark_artifact_paths(&report.database, report.evidence_log.as_deref(), path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create report directory {}", parent.display()))?;
    let file_name = path
        .file_name()
        .context("benchmark report path has no file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let write_result = (|| -> Result<()> {
        let mut file = File::create(&temporary)
            .with_context(|| format!("create temporary report {}", temporary.display()))?;
        serde_json::to_writer_pretty(&mut file, report).context("encode benchmark report")?;
        file.write_all(b"\n")
            .context("terminate benchmark report")?;
        file.sync_all().context("sync benchmark report")?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "publish benchmark report {} -> {}",
                temporary.display(),
                path.display()
            )
        })?;
        File::open(parent)
            .with_context(|| format!("open report directory {}", parent.display()))?
            .sync_all()
            .context("sync benchmark report directory")?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

/// Rejects benchmark artifacts that alias or live inside the measured database.
pub fn validate_benchmark_artifact_paths(
    database: &Path,
    evidence_log: Option<&Path>,
    report: &Path,
) -> Result<()> {
    let database = comparable_path(database)?;
    let report = comparable_path(report)?;
    if report.starts_with(&database) {
        bail!(
            "benchmark report {} must be outside measured database {}",
            report.display(),
            database.display()
        );
    }
    if let Some(evidence_log) = evidence_log {
        let evidence_log = comparable_path(evidence_log)?;
        if evidence_log.starts_with(&database) {
            bail!(
                "evidence log {} must be outside measured database {}",
                evidence_log.display(),
                database.display()
            );
        }
        if evidence_log == report {
            bail!(
                "benchmark report and evidence log resolve to the same path {}",
                report.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn comparable_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("benchmark artifact path must not be empty");
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolve current directory for benchmark artifact")?
            .join(path)
    };
    let mut existing = absolute;
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect benchmark path ancestor {}", existing.display())
                });
            }
        }
        let name = existing
            .file_name()
            .context("benchmark artifact path has no existing ancestor")?
            .to_os_string();
        suffix.push(name);
        if !existing.pop() {
            bail!("benchmark artifact path has no existing ancestor");
        }
    }
    let mut resolved = fs::canonicalize(&existing)
        .with_context(|| format!("canonicalize benchmark path {}", existing.display()))?;
    for name in suffix.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

fn require_durable_mdbx_sync_mode() -> Result<String> {
    let raw = std::env::var("NEO_MDBX_SYNC_MODE").unwrap_or_else(|_| "durable".to_owned());
    validate_durable_mdbx_sync_mode(&raw)
}

fn validate_durable_mdbx_sync_mode(raw: &str) -> Result<String> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', '_'], "");
    if matches!(normalized.as_str(), "durable" | "default") {
        return Ok("durable".to_owned());
    }
    bail!(
        "durable MDBX benchmark requires NEO_MDBX_SYNC_MODE to be unset, 'durable', or 'default'; got {raw:?}"
    )
}

/// Resolves the exact workload or a ratio-preserving bounded projection.
///
/// Backend bakeoffs use this shared function so MDBX and prototype stores
/// consume byte-identical operation streams for the same scale arguments.
pub fn resolve_shape(
    shape: WorkloadShape,
    scale: CampaignScale,
    smoke: SmokeSettings,
) -> Result<WorkloadShape> {
    if scale == CampaignScale::Full {
        return Ok(shape);
    }
    if smoke.prefill_rows == 0 || smoke.operations == 0 || smoke.blocks == 0 {
        bail!("smoke prefill, operations, and blocks must be greater than zero");
    }
    let full_operations = shape
        .put_count
        .checked_add(shape.tombstone_count)
        .context("full operation count overflows")?;
    let operations = smoke.operations.min(full_operations).max(1);
    let tombstones = scale_nearest(shape.tombstone_count, full_operations, operations);
    let puts = operations.saturating_sub(tombstones).max(1);
    let operations = puts + tombstones;
    let value_counts = apportion_counts(shape.value_sizes.map(|value| value.put_count), puts)?;
    let mut value_sizes = [ValueSizeClass::new(0, 0); 8];
    let mut lower_bound = 0u64;
    for index in 0..value_sizes.len() {
        let source = shape.value_sizes[index];
        let count = value_counts[index];
        let mut bytes = if count == 0 || source.put_count == 0 {
            0
        } else {
            scale_nearest(source.total_bytes, source.put_count, count)
        };
        let minimum = lower_bound.saturating_mul(count);
        let maximum =
            VALUE_SIZE_BUCKET_UPPER_BOUNDS[index].map(|upper| (upper as u64).saturating_mul(count));
        bytes = bytes.max(minimum);
        if let Some(maximum) = maximum {
            bytes = bytes.min(maximum);
        }
        value_sizes[index] = ValueSizeClass::new(count, bytes);
        lower_bound =
            VALUE_SIZE_BUCKET_UPPER_BOUNDS[index].map_or(16_385, |upper| upper as u64 + 1);
    }
    let projected_hits =
        scale_nearest(shape.version_hit_count, full_operations, operations).max(tombstones);
    let prefill_rows = smoke
        .prefill_rows
        .min(shape.prefill_rows)
        .max(projected_hits)
        .max(1);
    let blocks = smoke.blocks.min(shape.blocks).max(1);
    let commit_count = shape
        .commit_count
        .min(u32::try_from(blocks).unwrap_or(u32::MAX))
        .min(u32::try_from(operations).unwrap_or(u32::MAX))
        .max(1);
    let resolved = WorkloadShape {
        prefill_rows,
        blocks,
        commit_count,
        put_count: puts,
        tombstone_count: tombstones,
        version_hit_count: projected_hits,
        value_sizes,
        ..shape
    };
    crate::storage_workload::WorkloadCampaign::new(resolved)
        .map_err(anyhow::Error::new)
        .context("validate resolved smoke workload")?;
    Ok(resolved)
}

fn apportion_counts(weights: [u64; 8], target: u64) -> Result<[u64; 8]> {
    let total = weights.iter().try_fold(0u64, |total, value| {
        total
            .checked_add(*value)
            .context("value weight sum overflows")
    })?;
    if total == 0 {
        bail!("cannot scale an empty value distribution");
    }
    let mut counts = [0u64; 8];
    let mut remainders = [(0u128, 0usize); 8];
    let mut assigned = 0u64;
    for (index, weight) in weights.into_iter().enumerate() {
        let product = u128::from(weight) * u128::from(target);
        counts[index] = (product / u128::from(total)) as u64;
        remainders[index] = (product % u128::from(total), index);
        assigned += counts[index];
    }
    remainders
        .sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (_, index) in remainders
        .into_iter()
        .take(usize::try_from(target - assigned).unwrap_or(usize::MAX))
    {
        counts[index] += 1;
    }
    Ok(counts)
}

fn scale_nearest(value: u64, denominator: u64, target: u64) -> u64 {
    if denominator == 0 {
        return 0;
    }
    let numerator = u128::from(value) * u128::from(target);
    ((numerator + u128::from(denominator / 2)) / u128::from(denominator)) as u64
}

fn validate_nonempty_label(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} profile label must not be empty");
    }
    Ok(())
}

pub(crate) fn percentile_report(values: &[u64]) -> LatencyPercentiles {
    if values.is_empty() {
        return LatencyPercentiles::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let sum = sorted
        .iter()
        .fold(0u128, |sum, value| sum + u128::from(*value));
    LatencyPercentiles {
        samples: sorted.len() as u64,
        min_ns: sorted[0],
        mean_ns: (sum / sorted.len() as u128) as u64,
        p50_ns: nearest_rank(&sorted, 50),
        p95_ns: nearest_rank(&sorted, 95),
        p99_ns: nearest_rank(&sorted, 99),
        max_ns: sorted[sorted.len() - 1],
    }
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = (percentile * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_shape() -> WorkloadShape {
        WorkloadShape {
            source: "runner-test",
            seed: 7,
            prefill_rows: 100,
            blocks: 20,
            commit_count: 4,
            put_count: 90,
            tombstone_count: 10,
            version_hit_count: 20,
            value_sizes: [
                ValueSizeClass::new(50, 1_600),
                ValueSizeClass::new(40, 3_840),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
            ],
        }
    }

    #[test]
    fn smoke_projection_preserves_ratios_bounds_and_exact_bytes() {
        let shape = resolve_shape(
            test_shape(),
            CampaignScale::Smoke,
            SmokeSettings {
                prefill_rows: 30,
                operations: 20,
                blocks: 5,
            },
        )
        .expect("resolve smoke shape");
        assert_eq!(shape.prefill_rows, 30);
        assert_eq!(shape.put_count, 18);
        assert_eq!(shape.tombstone_count, 2);
        assert_eq!(shape.version_hit_count, 4);
        assert_eq!(shape.commit_count, 4);
        assert_eq!(
            shape.value_sizes.map(|value| value.put_count),
            [10, 8, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            shape.value_sizes.map(|value| value.total_bytes),
            [320, 768, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn nearest_rank_percentiles_are_stable_for_small_samples() {
        let report = percentile_report(&[100, 10, 50, 20, 90]);
        assert_eq!(report.samples, 5);
        assert_eq!(report.min_ns, 10);
        assert_eq!(report.mean_ns, 54);
        assert_eq!(report.p50_ns, 50);
        assert_eq!(report.p95_ns, 100);
        assert_eq!(report.p99_ns, 100);
        assert_eq!(report.max_ns, 100);
    }

    #[test]
    fn artifact_paths_reject_database_aliases_and_shared_output_files() {
        let temp = tempdir().expect("temporary artifact root");
        let database = temp.path().join("db");
        fs::create_dir(&database).expect("create database directory");
        let aliased_evidence = database.join("..").join("db").join("evidence.jsonl");
        let report = temp.path().join("report.json");
        let error = validate_benchmark_artifact_paths(&database, Some(&aliased_evidence), &report)
            .expect_err("aliased evidence path must be rejected");
        assert!(error.to_string().contains("outside measured database"));

        let shared = temp.path().join("shared.json");
        let error = validate_benchmark_artifact_paths(&database, Some(&shared), &shared)
            .expect_err("report must not replace evidence");
        assert!(error.to_string().contains("same path"));

        let inside_report = database.join("mdbx.dat");
        let error = validate_benchmark_artifact_paths(&database, None, &inside_report)
            .expect_err("report must not replace MDBX files");
        assert!(error.to_string().contains("outside measured database"));

        #[cfg(unix)]
        {
            let alias = temp.path().join("database-alias");
            std::os::unix::fs::symlink(&database, &alias).expect("create database symlink");
            let error = validate_benchmark_artifact_paths(&database, None, &alias.join("mdbx.dat"))
                .expect_err("symlinked report path must not enter MDBX");
            assert!(error.to_string().contains("outside measured database"));

            let dangling = temp.path().join("dangling-report");
            std::os::unix::fs::symlink(database.join("future.json"), &dangling)
                .expect("create dangling output symlink");
            validate_benchmark_artifact_paths(&database, None, &dangling)
                .expect_err("dangling output symlink must fail closed");
        }
    }

    #[test]
    fn durable_campaign_rejects_non_durable_or_ambiguous_sync_modes() {
        assert_eq!(
            validate_durable_mdbx_sync_mode(" durable ").expect("durable mode"),
            "durable"
        );
        assert_eq!(
            validate_durable_mdbx_sync_mode("DEFAULT").expect("default mode"),
            "durable"
        );
        for mode in ["no-meta-sync", "safe_no_sync", "utterly-no-sync", "typo"] {
            let error = validate_durable_mdbx_sync_mode(mode)
                .expect_err("non-durable or ambiguous mode must fail closed");
            assert!(error.to_string().contains("requires NEO_MDBX_SYNC_MODE"));
        }
    }
}

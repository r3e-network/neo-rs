use super::evidence::{
    EvidenceLog, RssSampler, capture_evidence, clock_ticks_per_second, evidence_delta,
};
use super::{
    BenchmarkBuildReport, DigestReport, EpochReport, LogicalVolume, MdbxBenchmarkConfig,
    MdbxBenchmarkReport, MdbxEnvironmentReport, MdbxMetricsDelta, MdbxRuntimeConfigReport,
    MetricDelta, PhaseReport, ReadBenchmarksReport, ReadLatencyReport, ReopenEpochCoverageReport,
    ReopenReport, ResolvedBenchmarkConfig, RunnerControlsReport, ThroughputRates, ThroughputReport,
    WorkloadReport, comparable_path, percentile_report,
};
use crate::storage_workload::{
    MPT_NODE_KEY_BYTES, OperationKind, VALUE_SIZE_BUCKET_UPPER_BOUNDS, WorkloadCampaign,
    WorkloadOperation,
};
use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_storage::mdbx::{
    MdbxCommitMetrics, MdbxCommitMetricsSnapshot, MdbxStore, MdbxStoreProvider,
};
use neo_storage::persistence::{RawReadOnlyStore, Store, storage::StorageConfig};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RSS_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
const MAX_PARALLEL_BATCH_READERS: usize = 16;
static RUNNER_LEASE: Mutex<()> = Mutex::new(());

/// Runs a fresh, durable MDBX campaign and returns a self-contained report.
pub fn run_mdbx_benchmark(config: &MdbxBenchmarkConfig) -> Result<MdbxBenchmarkReport> {
    let _lease = RUNNER_LEASE
        .lock()
        .map_err(|error| anyhow::anyhow!("MDBX benchmark process lease is poisoned: {error}"))?;
    let config = config.resolve()?;
    let build = build_report()?;
    let mdbx_runtime = mdbx_runtime_report(&config)?;
    prepare_database(&config.database)?;
    validate_evidence_path(&config)?;
    let run_started = Instant::now();
    let started_unix_ns = unix_time_ns();
    let clock_ticks = clock_ticks_per_second();
    let mut evidence_log = EvidenceLog::new(config.evidence_log.as_deref())?;
    let campaign = WorkloadCampaign::new(config.shape)
        .map_err(anyhow::Error::new)
        .context("construct resolved workload campaign")?;
    let store = open_store(&config.database, false)?;

    let requested_hits = config.point_queries.div_ceil(2);
    let mut verification = VerificationCorpus::new(requested_hits)?;
    let prefill_measurement =
        PhaseMeasurement::begin("prefill", &config.database, &mut evidence_log)?;
    let mut prefill_volume = LogicalVolume::default();
    let mut prefill_digest = OperationDigest::new(b"neo-mdbx-bench-prefill-v1");
    let mut prefill_commits = 0u64;
    let mut prefill = campaign.prefill();
    loop {
        let batch = prefill
            .by_ref()
            .take(config.prefill_batch_entries)
            .collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        for operation in &batch {
            prefill_volume.merge(observe_operation(operation)?)?;
            prefill_digest.observe(operation)?;
            verification.capture_prefill(operation)?;
        }
        commit_operations(&store, &batch).context("commit durable prefill batch")?;
        prefill_commits += 1;
    }
    let prefill_flush_started = Instant::now();
    store.flush().context("durably flush completed prefill")?;
    let _prefill_flush_ns = duration_ns(prefill_flush_started.elapsed());
    let prefill_report = prefill_measurement.finish(
        &config.database,
        &mut evidence_log,
        prefill_volume,
        clock_ticks,
    )?;
    ensure_metric_volume(&prefill_report, prefill_commits, "prefill")?;
    ensure!(
        verification.prefill_keys == verification.requested_prefill_keys,
        "prefill produced {} verification keys, expected {}",
        verification.prefill_keys,
        verification.requested_prefill_keys
    );

    let campaign_measurement =
        PhaseMeasurement::begin("campaign", &config.database, &mut evidence_log)?;
    let mut epochs = Vec::with_capacity(campaign.commits().len());
    let mut campaign_volume = LogicalVolume::default();
    let mut campaign_digest = OperationDigest::new(b"neo-mdbx-bench-campaign-v1");
    for batch in campaign.commits() {
        let phase_name = format!("campaign_epoch_{}", batch.commit_index);
        let measurement =
            PhaseMeasurement::begin(&phase_name, &config.database, &mut evidence_log)?;
        let operations = batch.operations().collect::<Vec<_>>();
        let mut logical = LogicalVolume::default();
        for operation in &operations {
            logical.merge(observe_operation(operation)?)?;
            campaign_digest.observe(operation)?;
        }
        commit_operations(&store, &operations)
            .with_context(|| format!("commit durable campaign epoch {}", batch.commit_index))?;
        verification.apply_campaign_epoch(batch.commit_index, &operations)?;
        let phase =
            measurement.finish(&config.database, &mut evidence_log, logical, clock_ticks)?;
        ensure_metric_volume(&phase, 1, &phase_name)?;
        campaign_volume.merge(logical)?;
        epochs.push(EpochReport {
            index: batch.commit_index,
            blocks: batch.blocks,
            phase,
        });
    }
    let final_flush_started = Instant::now();
    store.flush().context("final durable MDBX fence")?;
    let final_flush_ns = duration_ns(final_flush_started.elapsed());
    let campaign_report = campaign_measurement.finish(
        &config.database,
        &mut evidence_log,
        campaign_volume,
        clock_ticks,
    )?;
    ensure_metric_volume(
        &campaign_report,
        u64::from(config.shape.commit_count),
        "campaign",
    )?;
    ensure!(
        campaign_volume.entries == campaign.operation_count(),
        "generated campaign operation count does not match resolved shape"
    );
    ensure!(
        u128::from(campaign_volume.value_bytes) == campaign.logical_value_bytes(),
        "generated campaign value bytes do not match resolved shape"
    );
    ensure!(
        verification.campaign_keys >= verification.requested_campaign_keys,
        "campaign produced {} newly inserted verification keys, expected {}",
        verification.campaign_keys,
        verification.requested_campaign_keys
    );

    let read_queries = verification.read_queries(config.point_queries)?;
    let reopen_queries = verification.reopen_queries()?;
    let verification_coverage = verification_coverage(&reopen_queries)?;
    ensure!(
        verification_coverage.epoch_coverage.len()
            == usize::try_from(config.shape.commit_count)
                .context("commit count does not fit usize")?,
        "reopen verification covers {} epochs, expected {}",
        verification_coverage.epoch_coverage.len(),
        config.shape.commit_count
    );
    let reads = benchmark_reads(&store, &read_queries, &config)?;
    let environment = environment_report(&store)?;
    drop(store);

    let expected_reopen_digest = verification_digest_expected(&reopen_queries)?;
    let reopen_started = Instant::now();
    let reopened = open_store(&config.database, true).context("reopen MDBX read-only")?;
    let reopen_ns = duration_ns(reopen_started.elapsed());
    let (actual_reopen_digest, verified_keys) = verify_reopened(&reopened, &reopen_queries)?;
    ensure!(
        expected_reopen_digest == actual_reopen_digest,
        "reopened verification digest mismatch: expected {expected_reopen_digest}, actual {actual_reopen_digest}"
    );
    let reopen_environment = environment_report(&reopened)?;
    ensure!(
        reopen_environment.transaction_id == environment.transaction_id,
        "reopened MDBX transaction id {} does not match pre-drop transaction id {}",
        reopen_environment.transaction_id,
        environment.transaction_id
    );
    let final_snapshot = capture_evidence(&config.database)?;
    evidence_log.checkpoint("reopen", "verified", &final_snapshot)?;
    drop(reopened);
    let measured_campaign_wall_ns = epochs.iter().try_fold(0u64, |total, epoch| {
        total
            .checked_add(epoch.phase.wall_ns)
            .context("measured campaign wall time overflows u64")
    })?;
    let campaign_throughput = throughput_report(
        config.shape.blocks,
        &campaign_report,
        measured_campaign_wall_ns,
    )?;

    let report = MdbxBenchmarkReport {
        schema_version: 1,
        status: "completed".to_owned(),
        backend: "mdbx".to_owned(),
        pid: std::process::id(),
        started_unix_ns,
        finished_unix_ns: unix_time_ns(),
        total_wall_ns: duration_ns(run_started.elapsed()),
        clock_ticks_per_second: clock_ticks,
        effective_mdbx_sync_mode: config.effective_mdbx_sync_mode,
        scale: config.scale,
        labels: config.labels,
        build,
        mdbx_runtime,
        database: config.database,
        evidence_log: config.evidence_log,
        workload: WorkloadReport::from(config.shape),
        controls: RunnerControlsReport {
            prefill_batch_entries: config.prefill_batch_entries,
            point_queries: config.point_queries,
            point_rounds: config.point_rounds,
            sorted_batch_keys: config.sorted_batch_keys,
            sorted_batch_rounds: config.sorted_batch_rounds,
            smoke: config.smoke,
        },
        prefill: prefill_report,
        epochs,
        campaign: campaign_report,
        campaign_throughput,
        final_flush_ns,
        reads,
        reopen: ReopenReport {
            open_ns: reopen_ns,
            verified_keys,
            prefill_keys: verification_coverage.prefill_keys,
            campaign_keys: verification_coverage.campaign_keys,
            version_hit_puts: verification_coverage.version_hit_puts,
            tombstones: verification_coverage.tombstones,
            absent_keys: verification_coverage.absent_keys,
            epoch_coverage: verification_coverage.epoch_coverage,
            expected_digest: expected_reopen_digest.clone(),
            actual_digest: actual_reopen_digest,
            matched: true,
            pre_drop_transaction_id: environment.transaction_id,
            transaction_id: reopen_environment.transaction_id,
        },
        digests: DigestReport {
            prefill_sha256: prefill_digest.finish(),
            campaign_sha256: campaign_digest.finish(),
        },
        environment,
        evidence_limitations: vec![
            "/proc/self/io write_bytes is process-attributed storage-layer traffic, not complete filesystem journal or device traffic".to_owned(),
            "allocated file bytes measure retained footprint, not rewrite traffic".to_owned(),
            "VmHWM is process-lifetime high water; timed peaks use the bounded 50 ms sampler".to_owned(),
            "device-level promotion evidence requires an isolated volume or correlated external block/eBPF/NVMe counters".to_owned(),
            "process I/O includes the optional evidence JSONL stream when configured".to_owned(),
            "process CPU, RSS, I/O, and MDBX metric families are process-global; the runner serializes its own campaigns but promotion runs require an otherwise isolated process".to_owned(),
            "ingestion throughput includes deterministic generation, allocation, accounting, hashing, backend commit, and post-commit sample maintenance; backend and durable-commit rates use MDBX-owned stage timers".to_owned(),
        ],
    };
    Ok(report)
}

struct PhaseMeasurement {
    name: String,
    database: std::path::PathBuf,
    before: super::EvidenceSnapshot,
    mdbx_before: MdbxCommitMetricsSnapshot,
    started: Instant,
    rss: RssSampler,
}

impl PhaseMeasurement {
    fn begin(name: &str, database: &Path, evidence_log: &mut EvidenceLog) -> Result<Self> {
        let before = capture_evidence(database)?;
        evidence_log.checkpoint(name, "before", &before)?;
        Ok(Self {
            name: name.to_owned(),
            database: database.to_path_buf(),
            before,
            mdbx_before: MdbxCommitMetrics::snapshot(),
            started: Instant::now(),
            rss: RssSampler::start(RSS_SAMPLE_INTERVAL),
        })
    }

    fn finish(
        self,
        database: &Path,
        evidence_log: &mut EvidenceLog,
        logical: LogicalVolume,
        clock_ticks: Option<u64>,
    ) -> Result<PhaseReport> {
        ensure!(
            self.database == database,
            "phase database path changed during measurement"
        );
        let wall = self.started.elapsed();
        let after = capture_evidence(database)?;
        let mdbx_after = MdbxCommitMetrics::snapshot();
        let sampled_rss = self.rss.finish();
        evidence_log.checkpoint(&self.name, "after", &after)?;
        let delta = evidence_delta(&self.before, &after, wall, clock_ticks);
        let write_bytes = delta.process.write_bytes;
        let mdbx = metrics_delta(&self.mdbx_before, &mdbx_after)?;
        Ok(PhaseReport {
            name: self.name,
            wall_ns: duration_ns(wall),
            logical,
            before: self.before,
            after,
            evidence_delta: delta,
            sampled_rss,
            mdbx,
            process_physical_write_bytes: write_bytes,
            write_amplification_vs_values: ratio(write_bytes, logical.value_bytes),
            write_amplification_vs_mutations: ratio(write_bytes, logical.mutation_bytes),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CampaignMutationKind {
    NewPut,
    VersionHitPut,
    Tombstone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CampaignSampleTag {
    epoch: u32,
    kind: CampaignMutationKind,
}

#[derive(Clone)]
struct VerificationSample {
    key: [u8; MPT_NODE_KEY_BYTES],
    expected: Option<Vec<u8>>,
    from_prefill: bool,
    new_campaign_put: bool,
    campaign_tags: Vec<CampaignSampleTag>,
    synthetic_absent: bool,
}

struct VerificationCorpus {
    requested_prefill_keys: usize,
    requested_campaign_keys: usize,
    prefill_keys: usize,
    campaign_keys: usize,
    hits: Vec<VerificationSample>,
    hit_indices: HashMap<[u8; MPT_NODE_KEY_BYTES], usize>,
}

impl VerificationCorpus {
    fn new(requested_hits: usize) -> Result<Self> {
        let capacity = requested_hits
            .checked_mul(2)
            .context("verification corpus capacity overflows usize")?;
        Ok(Self {
            requested_prefill_keys: requested_hits,
            requested_campaign_keys: requested_hits,
            prefill_keys: 0,
            campaign_keys: 0,
            hits: Vec::with_capacity(capacity),
            hit_indices: HashMap::with_capacity(capacity),
        })
    }

    fn capture_prefill(&mut self, operation: &WorkloadOperation) -> Result<()> {
        if self.prefill_keys >= self.requested_prefill_keys {
            return Ok(());
        }
        let OperationKind::Put(value) = &operation.kind else {
            bail!("prefill stream emitted a tombstone");
        };
        let index = self.hits.len();
        ensure!(
            self.hit_indices.insert(operation.key, index).is_none(),
            "prefill generated a duplicate verification key"
        );
        self.hits.push(VerificationSample {
            key: operation.key,
            expected: Some(value.clone()),
            from_prefill: true,
            new_campaign_put: false,
            campaign_tags: Vec::new(),
            synthetic_absent: false,
        });
        self.prefill_keys = self
            .prefill_keys
            .checked_add(1)
            .context("prefill verification count overflows usize")?;
        Ok(())
    }

    fn apply_campaign_epoch(&mut self, epoch: u32, operations: &[WorkloadOperation]) -> Result<()> {
        for kind in [
            CampaignMutationKind::NewPut,
            CampaignMutationKind::VersionHitPut,
            CampaignMutationKind::Tombstone,
        ] {
            if let Some(operation) = operations
                .iter()
                .find(|operation| campaign_mutation_kind(operation) == kind)
            {
                self.ensure_campaign_sample(epoch, kind, operation)?;
            }
        }

        if self.campaign_keys < self.requested_campaign_keys {
            for operation in operations.iter().filter(|operation| {
                campaign_mutation_kind(operation) == CampaignMutationKind::NewPut
            }) {
                self.ensure_campaign_sample(epoch, CampaignMutationKind::NewPut, operation)?;
                if self.campaign_keys >= self.requested_campaign_keys {
                    break;
                }
            }
        }

        for operation in operations {
            if let Some(index) = self.hit_indices.get(&operation.key).copied() {
                self.hits[index].expected = operation_value(operation);
            }
        }
        Ok(())
    }

    fn ensure_campaign_sample(
        &mut self,
        epoch: u32,
        kind: CampaignMutationKind,
        operation: &WorkloadOperation,
    ) -> Result<()> {
        let index = if let Some(index) = self.hit_indices.get(&operation.key).copied() {
            index
        } else {
            let index = self.hits.len();
            self.hit_indices.insert(operation.key, index);
            self.hits.push(VerificationSample {
                key: operation.key,
                expected: operation_value(operation),
                from_prefill: false,
                new_campaign_put: false,
                campaign_tags: Vec::new(),
                synthetic_absent: false,
            });
            index
        };
        let sample = &mut self.hits[index];
        if kind == CampaignMutationKind::NewPut && !sample.new_campaign_put {
            sample.new_campaign_put = true;
            self.campaign_keys = self
                .campaign_keys
                .checked_add(1)
                .context("campaign verification count overflows usize")?;
        }
        let tag = CampaignSampleTag { epoch, kind };
        if !sample.campaign_tags.contains(&tag) {
            sample.campaign_tags.push(tag);
        }
        Ok(())
    }

    fn read_queries(&self, count: usize) -> Result<Vec<VerificationSample>> {
        let hit_count = count.div_ceil(2);
        let prefill_target = hit_count / 2;
        let campaign_target = hit_count - prefill_target;
        let mut selected = Vec::with_capacity(hit_count);
        let mut selected_indices = HashSet::with_capacity(hit_count);
        self.select_present_samples(
            &mut selected,
            &mut selected_indices,
            prefill_target,
            |sample| sample.from_prefill,
        )?;
        self.select_present_samples(
            &mut selected,
            &mut selected_indices,
            campaign_target,
            |sample| sample.new_campaign_put,
        )?;
        if selected.len() < hit_count {
            let remaining = hit_count - selected.len();
            self.select_present_samples(&mut selected, &mut selected_indices, remaining, |_| true)?;
        }
        ensure!(
            selected.len() == hit_count,
            "verification corpus has {} present samples, expected {hit_count}",
            selected.len()
        );
        let mut queries = Vec::with_capacity(count);
        for index in selected {
            let hit = &self.hits[index];
            queries.push(hit.clone());
            if queries.len() == count {
                break;
            }
            queries.push(missing_sample(hit));
            if queries.len() == count {
                break;
            }
        }
        ensure!(
            queries.len() == count,
            "failed to construct exact read corpus"
        );
        Ok(queries)
    }

    fn select_present_samples(
        &self,
        selected: &mut Vec<usize>,
        selected_indices: &mut HashSet<usize>,
        count: usize,
        predicate: impl Fn(&VerificationSample) -> bool,
    ) -> Result<()> {
        let target = selected
            .len()
            .checked_add(count)
            .context("read verification selection target overflows usize")?;
        for (index, sample) in self.hits.iter().enumerate() {
            if selected.len() >= target {
                break;
            }
            if sample.expected.is_some() && predicate(sample) && selected_indices.insert(index) {
                selected.push(index);
            }
        }
        Ok(())
    }

    fn reopen_queries(&self) -> Result<Vec<VerificationSample>> {
        let capacity = self
            .hits
            .len()
            .checked_mul(2)
            .context("reopen verification corpus size overflows usize")?;
        let mut queries = Vec::with_capacity(capacity);
        for hit in &self.hits {
            queries.push(hit.clone());
            queries.push(missing_sample(hit));
        }
        Ok(queries)
    }
}

fn campaign_mutation_kind(operation: &WorkloadOperation) -> CampaignMutationKind {
    match (&operation.kind, operation.version_hit) {
        (OperationKind::Tombstone, _) => CampaignMutationKind::Tombstone,
        (OperationKind::Put(_), true) => CampaignMutationKind::VersionHitPut,
        (OperationKind::Put(_), false) => CampaignMutationKind::NewPut,
    }
}

fn operation_value(operation: &WorkloadOperation) -> Option<Vec<u8>> {
    match &operation.kind {
        OperationKind::Put(value) => Some(value.clone()),
        OperationKind::Tombstone => None,
    }
}

fn missing_sample(hit: &VerificationSample) -> VerificationSample {
    let mut key = hit.key;
    key[0] = key[0].wrapping_add(1);
    VerificationSample {
        key,
        expected: None,
        from_prefill: false,
        new_campaign_put: false,
        campaign_tags: Vec::new(),
        synthetic_absent: true,
    }
}

#[derive(Clone)]
struct VerificationCoverage {
    prefill_keys: u64,
    campaign_keys: u64,
    version_hit_puts: u64,
    tombstones: u64,
    absent_keys: u64,
    epoch_coverage: Vec<ReopenEpochCoverageReport>,
}

fn verification_coverage(queries: &[VerificationSample]) -> Result<VerificationCoverage> {
    let count = |predicate: fn(&VerificationSample) -> bool| -> Result<u64> {
        queries
            .iter()
            .filter(|query| predicate(query))
            .count()
            .try_into()
            .context("verification corpus count does not fit u64")
    };
    let mut epochs = BTreeMap::<u32, ReopenEpochCoverageReport>::new();
    let mut version_hit_puts = 0u64;
    let mut tombstones = 0u64;
    for tag in queries.iter().flat_map(|query| &query.campaign_tags) {
        let epoch = epochs
            .entry(tag.epoch)
            .or_insert(ReopenEpochCoverageReport {
                epoch: tag.epoch,
                ..ReopenEpochCoverageReport::default()
            });
        match tag.kind {
            CampaignMutationKind::NewPut => {
                epoch.new_puts = epoch
                    .new_puts
                    .checked_add(1)
                    .context("reopen new-put coverage overflows u64")?;
            }
            CampaignMutationKind::VersionHitPut => {
                epoch.version_hit_puts = epoch
                    .version_hit_puts
                    .checked_add(1)
                    .context("reopen version-hit coverage overflows u64")?;
                version_hit_puts = version_hit_puts
                    .checked_add(1)
                    .context("reopen version-hit total overflows u64")?;
            }
            CampaignMutationKind::Tombstone => {
                epoch.tombstones = epoch
                    .tombstones
                    .checked_add(1)
                    .context("reopen tombstone coverage overflows u64")?;
                tombstones = tombstones
                    .checked_add(1)
                    .context("reopen tombstone total overflows u64")?;
            }
        }
    }
    Ok(VerificationCoverage {
        prefill_keys: count(|query| query.from_prefill)?,
        campaign_keys: count(|query| query.new_campaign_put)?,
        version_hit_puts,
        tombstones,
        absent_keys: count(|query| query.synthetic_absent)?,
        epoch_coverage: epochs.into_values().collect(),
    })
}

fn benchmark_reads(
    store: &MdbxStore,
    queries: &[VerificationSample],
    config: &ResolvedBenchmarkConfig,
) -> Result<ReadBenchmarksReport> {
    let snapshot = store.snapshot();
    let point_rounds = usize::try_from(config.point_rounds)
        .context("point benchmark round count does not fit usize")?;
    let point_capacity = queries
        .len()
        .checked_mul(point_rounds)
        .context("point latency sample count overflows usize")?;
    let mut point_calls = Vec::with_capacity(point_capacity);
    let mut point_bytes = 0u64;
    for _ in 0..config.point_rounds {
        for query in queries {
            let started = Instant::now();
            let actual = snapshot
                .try_get_bytes_result(&query.key)
                .context("MDBX point lookup")?;
            point_calls.push(duration_ns(started.elapsed()));
            ensure!(
                actual == query.expected,
                "point lookup returned an unexpected value"
            );
            let value_bytes = actual
                .as_ref()
                .map_or(Ok(0), |value| u64::try_from(value.len()))
                .context("point lookup byte count does not fit u64")?;
            point_bytes = point_bytes
                .checked_add(value_bytes)
                .context("point lookup byte total overflows u64")?;
            std::hint::black_box(actual);
        }
    }

    let mut sorted = queries.to_vec();
    sorted.sort_unstable_by_key(|sample| sample.key);
    let mut batch_calls = Vec::new();
    let mut batch_per_key = Vec::new();
    let mut batch_bytes = 0u64;
    let mut min_batch_keys = usize::MAX;
    let mut max_batch_keys = 0usize;
    for _ in 0..config.sorted_batch_rounds {
        for chunk in sorted.chunks(config.sorted_batch_keys) {
            min_batch_keys = min_batch_keys.min(chunk.len());
            max_batch_keys = max_batch_keys.max(chunk.len());
            let keys = chunk.iter().map(|sample| sample.key).collect::<Vec<_>>();
            let started = Instant::now();
            let actual = snapshot
                .try_get_many_bytes_sorted(&keys)
                .context("MDBX sorted batch lookup")?;
            let elapsed = duration_ns(started.elapsed());
            batch_calls.push(elapsed);
            let chunk_len =
                u64::try_from(chunk.len()).context("sorted batch key count does not fit u64")?;
            batch_per_key.push(elapsed / chunk_len);
            ensure!(
                actual.len() == chunk.len(),
                "sorted lookup changed result count"
            );
            for (actual, expected) in actual.iter().zip(chunk) {
                ensure!(
                    actual == &expected.expected,
                    "sorted lookup returned an unexpected value"
                );
                let value_bytes = actual
                    .as_ref()
                    .map_or(Ok(0), |value| u64::try_from(value.len()))
                    .context("sorted lookup byte count does not fit u64")?;
                batch_bytes = batch_bytes
                    .checked_add(value_bytes)
                    .context("sorted lookup byte total overflows u64")?;
            }
            std::hint::black_box(actual);
        }
    }
    let query_count = u64::try_from(queries.len()).context("read corpus size does not fit u64")?;
    let hits = u64::try_from(
        queries
            .iter()
            .filter(|query| query.expected.is_some())
            .count(),
    )
    .context("read hit count does not fit u64")?;
    let misses = query_count
        .checked_sub(hits)
        .context("read hit count exceeds corpus size")?;
    let point_total = query_count
        .checked_mul(u64::from(config.point_rounds))
        .context("point lookup key total overflows u64")?;
    let batch_total = query_count
        .checked_mul(u64::from(config.sorted_batch_rounds))
        .context("sorted lookup key total overflows u64")?;
    let point_hits = hits
        .checked_mul(u64::from(config.point_rounds))
        .context("point lookup hit total overflows u64")?;
    let point_misses = misses
        .checked_mul(u64::from(config.point_rounds))
        .context("point lookup miss total overflows u64")?;
    let batch_hits = hits
        .checked_mul(u64::from(config.sorted_batch_rounds))
        .context("sorted lookup hit total overflows u64")?;
    let batch_misses = misses
        .checked_mul(u64::from(config.sorted_batch_rounds))
        .context("sorted lookup miss total overflows u64")?;
    Ok(ReadBenchmarksReport {
        percentile_method: "nearest_rank".to_owned(),
        point: ReadLatencyReport {
            mode: "point".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.point_rounds,
            keys: point_total,
            hits: point_hits,
            misses: point_misses,
            value_bytes: point_bytes,
            target_keys_per_call: 1,
            min_keys_per_call: 1,
            max_keys_per_call: 1,
            call_latency: percentile_report(&point_calls),
            normalized_per_key_latency: percentile_report(&point_calls),
        },
        sorted_batch: ReadLatencyReport {
            mode: "sorted_batch".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.sorted_batch_rounds,
            keys: batch_total,
            hits: batch_hits,
            misses: batch_misses,
            value_bytes: batch_bytes,
            target_keys_per_call: config.sorted_batch_keys,
            min_keys_per_call: min_batch_keys,
            max_keys_per_call: max_batch_keys,
            call_latency: percentile_report(&batch_calls),
            normalized_per_key_latency: percentile_report(&batch_per_key),
        },
    })
}

fn verify_reopened(store: &MdbxStore, queries: &[VerificationSample]) -> Result<(String, u64)> {
    let snapshot = store.snapshot();
    let mut digest = Sha256Hasher::new();
    digest.update(b"neo-mdbx-bench-reopen-v1");
    for query in queries {
        let actual = snapshot
            .try_get_bytes_result(&query.key)
            .context("read reopened verification key")?;
        ensure!(actual == query.expected, "reopened MDBX value mismatch");
        digest_verification_value(&mut digest, &query.key, actual.as_deref())?;
    }
    Ok((
        hex::encode(digest.finalize()),
        queries
            .len()
            .try_into()
            .context("reopen verification count does not fit u64")?,
    ))
}

fn verification_digest_expected(queries: &[VerificationSample]) -> Result<String> {
    let mut digest = Sha256Hasher::new();
    digest.update(b"neo-mdbx-bench-reopen-v1");
    for query in queries {
        digest_verification_value(&mut digest, &query.key, query.expected.as_deref())?;
    }
    Ok(hex::encode(digest.finalize()))
}

fn digest_verification_value(
    digest: &mut Sha256Hasher,
    key: &[u8; MPT_NODE_KEY_BYTES],
    value: Option<&[u8]>,
) -> Result<()> {
    digest.update(key);
    match value {
        Some(value) => {
            digest.update(&[1]);
            let value_len =
                u64::try_from(value.len()).context("verification value length does not fit u64")?;
            digest.update(&value_len.to_le_bytes());
            digest.update(value);
        }
        None => digest.update(&[0]),
    }
    Ok(())
}

struct OperationDigest(Sha256Hasher);

impl OperationDigest {
    fn new(domain: &[u8]) -> Self {
        let mut digest = Sha256Hasher::new();
        digest.update(domain);
        Self(digest)
    }

    fn observe(&mut self, operation: &WorkloadOperation) -> Result<()> {
        self.0.update(&operation.key);
        self.0.update(&[u8::from(operation.version_hit)]);
        match &operation.kind {
            OperationKind::Put(value) => {
                self.0.update(&[1]);
                let value_len = u64::try_from(value.len())
                    .context("operation digest value length does not fit u64")?;
                self.0.update(&value_len.to_le_bytes());
                self.0.update(value);
            }
            OperationKind::Tombstone => self.0.update(&[0]),
        }
        Ok(())
    }

    fn finish(self) -> String {
        hex::encode(self.0.finalize())
    }
}

fn observe_operation(operation: &WorkloadOperation) -> Result<LogicalVolume> {
    let key_bytes = operation
        .key
        .len()
        .try_into()
        .context("operation key length does not fit u64")?;
    Ok(match &operation.kind {
        OperationKind::Put(value) => {
            let value_bytes = value
                .len()
                .try_into()
                .context("operation value length does not fit u64")?;
            let bucket = VALUE_SIZE_BUCKET_UPPER_BOUNDS
                .iter()
                .position(|upper| upper.is_none_or(|upper| value.len() <= upper))
                .context("operation value does not fit a value-size bucket")?;
            let mut value_size_counts = [0u64; 8];
            value_size_counts[bucket] = 1;
            LogicalVolume {
                puts: 1,
                tombstones: 0,
                entries: 1,
                key_bytes,
                value_bytes,
                mutation_bytes: key_bytes
                    .checked_add(value_bytes)
                    .context("operation mutation byte count overflows")?,
                value_size_counts,
            }
        }
        OperationKind::Tombstone => LogicalVolume {
            puts: 0,
            tombstones: 1,
            entries: 1,
            key_bytes,
            value_bytes: 0,
            mutation_bytes: key_bytes,
            value_size_counts: [0; 8],
        },
    })
}

fn commit_operations(store: &MdbxStore, operations: &[WorkloadOperation]) -> Result<()> {
    store
        .commit_raw_overlay(operations.iter().map(|operation| match &operation.kind {
            OperationKind::Put(value) => (operation.key.as_slice(), Some(value.as_slice())),
            OperationKind::Tombstone => (operation.key.as_slice(), None),
        }))
        .map_err(anyhow::Error::new)
}

fn build_report() -> Result<BenchmarkBuildReport> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("benchmark manifest directory has no repository parent")?;
    let git_revision = git_output(repository, &["rev-parse", "HEAD"]);
    let git_dirty = git_output(repository, &["status", "--short", "--untracked-files=all"])
        .map(|status| !status.is_empty());
    let executable = std::env::current_exe()
        .context("resolve benchmark executable")?
        .canonicalize()
        .context("canonicalize benchmark executable")?;
    Ok(BenchmarkBuildReport {
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        profile: option_env!("NEO_BENCH_BUILD_PROFILE")
            .unwrap_or("unavailable")
            .to_owned(),
        opt_level: option_env!("NEO_BENCH_BUILD_OPT_LEVEL")
            .unwrap_or("unavailable")
            .to_owned(),
        debug_assertions: cfg!(debug_assertions),
        target: option_env!("NEO_BENCH_BUILD_TARGET")
            .unwrap_or("unavailable")
            .to_owned(),
        host: option_env!("NEO_BENCH_BUILD_HOST")
            .unwrap_or("unavailable")
            .to_owned(),
        rustc: option_env!("NEO_BENCH_BUILD_RUSTC")
            .unwrap_or("unavailable")
            .to_owned(),
        executable,
        git_revision,
        git_dirty,
    })
}

fn git_output(repository: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn mdbx_runtime_report(config: &ResolvedBenchmarkConfig) -> Result<MdbxRuntimeConfigReport> {
    let tuning = MdbxStoreProvider::new(StorageConfig {
        path: config.database.clone(),
        ..StorageConfig::default()
    })
    .tuning();
    let batch_read_threads = configured_threads("NEO_MDBX_BATCH_READ_THREADS", 1);
    let write_intent_read_threads = match std::env::var("NEO_MDBX_WRITE_INTENT_READ_THREADS") {
        Ok(_) => configured_threads("NEO_MDBX_WRITE_INTENT_READ_THREADS", 1),
        Err(_) => batch_read_threads,
    };
    Ok(MdbxRuntimeConfigReport {
        sync_mode: config.effective_mdbx_sync_mode.clone(),
        cursor_write_mode: configured_cursor_write_mode(),
        coalesce: configured_flag("NEO_MDBX_COALESCE"),
        no_meminit: configured_flag("NEO_MDBX_NO_MEMINIT"),
        prefix_index_path: std::env::var_os("NEO_MDBX_PREFIX_INDEX_PATH").map(Into::into),
        batch_read_threads,
        write_intent_read_threads,
        geometry_upper_bytes: tuning
            .map_size
            .try_into()
            .context("MDBX geometry upper bound does not fit u64")?,
        geometry_growth_bytes: tuning
            .growth_step
            .try_into()
            .context("MDBX geometry growth step does not fit u64")?,
        requested_max_readers: tuning.max_readers,
    })
}

fn configured_cursor_write_mode() -> String {
    let normalized = std::env::var("NEO_MDBX_CURSOR_WRITE_MODE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "");
    if matches!(normalized.as_str(), "merge" | "mergecursor" | "cursormerge") {
        "merge".to_owned()
    } else {
        "search".to_owned()
    }
}

fn configured_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn configured_threads(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(fallback)
        .clamp(1, MAX_PARALLEL_BATCH_READERS)
}

fn open_store(path: &Path, read_only: bool) -> Result<MdbxStore> {
    MdbxStoreProvider::new(StorageConfig {
        path: path.to_path_buf(),
        read_only,
        ..Default::default()
    })
    .get_mdbx_store("")
    .map_err(anyhow::Error::new)
    .with_context(|| format!("open MDBX store {} read_only={read_only}", path.display()))
}

fn prepare_database(path: &Path) -> Result<()> {
    if path.exists() {
        ensure!(
            path.is_dir(),
            "database path {} is not a directory",
            path.display()
        );
        ensure!(
            fs::read_dir(path)
                .with_context(|| format!("read database directory {}", path.display()))?
                .next()
                .is_none(),
            "database directory {} is not empty; refusing to overwrite benchmark evidence",
            path.display()
        );
    } else {
        fs::create_dir_all(path)
            .with_context(|| format!("create benchmark database {}", path.display()))?;
    }
    Ok(())
}

fn validate_evidence_path(config: &ResolvedBenchmarkConfig) -> Result<()> {
    if let Some(evidence) = config.evidence_log.as_deref() {
        let database = comparable_path(&config.database)?;
        let evidence = comparable_path(evidence)?;
        ensure!(
            !evidence.starts_with(&database),
            "evidence log {} must be outside the measured database tree {}",
            evidence.display(),
            database.display()
        );
    }
    Ok(())
}

fn environment_report(store: &MdbxStore) -> Result<MdbxEnvironmentReport> {
    let info = store.info().map_err(anyhow::Error::new)?;
    Ok(MdbxEnvironmentReport {
        map_size: info
            .map_size()
            .try_into()
            .context("MDBX map size does not fit u64")?,
        last_page_number: info
            .last_pgno()
            .try_into()
            .context("MDBX last page number does not fit u64")?,
        transaction_id: info
            .last_txnid()
            .try_into()
            .context("MDBX transaction id does not fit u64")?,
        max_readers: info
            .max_readers()
            .try_into()
            .context("MDBX max reader count does not fit u64")?,
        readers: info
            .num_readers()
            .try_into()
            .context("MDBX reader count does not fit u64")?,
    })
}

fn metrics_delta(
    before: &MdbxCommitMetricsSnapshot,
    after: &MdbxCommitMetricsSnapshot,
) -> Result<MdbxMetricsDelta> {
    let stage_before = before
        .stages
        .iter()
        .map(|stage| (stage.stage, (stage.calls, stage.total_us)))
        .collect::<HashMap<_, _>>();
    let count_before = before
        .counts
        .iter()
        .map(|count| (count.kind, (count.samples, count.total)))
        .collect::<HashMap<_, _>>();
    let stages = after
        .stages
        .iter()
        .map(|stage| {
            let (calls, total) = stage_before.get(stage.stage).copied().unwrap_or_default();
            Ok(MetricDelta {
                name: stage.stage.to_owned(),
                samples: metric_counter_delta(stage.stage, "samples", calls, stage.calls)?,
                total: metric_counter_delta(stage.stage, "total", total, stage.total_us)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let durable_commit_us = stages
        .iter()
        .find(|stage| stage.name == "commit")
        .context("MDBX metrics are missing the durable commit stage")?
        .total;
    Ok(MdbxMetricsDelta {
        attempts: metric_counter_delta(
            "commit",
            "attempts",
            before.stats.attempts,
            after.stats.attempts,
        )?,
        failures: metric_counter_delta(
            "commit",
            "failures",
            before.stats.failures,
            after.stats.failures,
        )?,
        committed_transactions: metric_counter_delta(
            "commit",
            "committed transactions",
            before.stats.committed_transactions,
            after.stats.committed_transactions,
        )?,
        durable_commit_us,
        stages,
        counts: after
            .counts
            .iter()
            .map(|count| {
                let (samples, total) = count_before.get(count.kind).copied().unwrap_or_default();
                Ok(MetricDelta {
                    name: count.kind.to_owned(),
                    samples: metric_counter_delta(count.kind, "samples", samples, count.samples)?,
                    total: metric_counter_delta(count.kind, "total", total, count.total)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

fn metric_counter_delta(name: &str, field: &str, before: u64, after: u64) -> Result<u64> {
    after
        .checked_sub(before)
        .with_context(|| format!("MDBX metric {name} {field} regressed from {before} to {after}"))
}

fn ensure_metric_volume(phase: &PhaseReport, commits: u64, label: &str) -> Result<()> {
    ensure!(
        phase.mdbx.attempts == commits,
        "{label} attempted {} MDBX transactions, expected {commits}",
        phase.mdbx.attempts
    );
    ensure!(
        phase.mdbx.failures == 0,
        "{label} recorded {} failed MDBX transactions",
        phase.mdbx.failures
    );
    ensure!(
        phase.mdbx.committed_transactions == commits,
        "{label} committed {} MDBX transactions, expected {commits}",
        phase.mdbx.committed_transactions
    );
    let durable_commit = phase
        .mdbx
        .stages
        .iter()
        .find(|stage| stage.name == "commit")
        .with_context(|| format!("{label} is missing MDBX durable commit timing"))?;
    ensure!(
        durable_commit.samples == commits,
        "{label} recorded {} durable commit samples, expected {commits}",
        durable_commit.samples
    );
    ensure!(
        durable_commit.total == phase.mdbx.durable_commit_us,
        "{label} durable commit summary does not match commit stage"
    );
    for (kind, expected) in [
        ("entries", phase.logical.entries),
        ("puts", phase.logical.puts),
        ("deletes", phase.logical.tombstones),
        ("key_bytes", phase.logical.key_bytes),
        ("value_bytes", phase.logical.value_bytes),
    ] {
        let actual = phase
            .mdbx
            .counts
            .iter()
            .find(|count| count.name == kind)
            .with_context(|| format!("{label} is missing MDBX {kind} metrics"))?
            .total;
        ensure!(
            actual == expected,
            "{label} MDBX {kind} metric {actual} does not match exact logical {expected}"
        );
    }
    for (kind, expected) in [
        "value_size_0_64",
        "value_size_65_128",
        "value_size_129_256",
        "value_size_257_512",
        "value_size_513_1024",
        "value_size_1025_4096",
        "value_size_4097_16384",
        "value_size_over_16384",
    ]
    .into_iter()
    .zip(phase.logical.value_size_counts)
    {
        let actual = phase
            .mdbx
            .counts
            .iter()
            .find(|count| count.name == kind)
            .with_context(|| format!("{label} is missing MDBX {kind} metrics"))?
            .total;
        ensure!(
            actual == expected,
            "{label} MDBX {kind} metric {actual} does not match exact logical {expected}"
        );
    }
    Ok(())
}

fn ratio(numerator: Option<u64>, denominator: u64) -> Option<f64> {
    numerator.and_then(|numerator| (denominator > 0).then(|| numerator as f64 / denominator as f64))
}

fn throughput_report(
    blocks: u64,
    phase: &PhaseReport,
    ingestion_wall_ns: u64,
) -> Result<ThroughputReport> {
    let backend_commit_us = phase
        .mdbx
        .stages
        .iter()
        .find(|stage| stage.name == "total")
        .context("campaign MDBX metrics are missing the total commit stage")?
        .total;
    let backend_commit_ns = backend_commit_us
        .checked_mul(1_000)
        .context("backend commit duration overflows nanoseconds")?;
    let durable_commit_ns = phase
        .mdbx
        .durable_commit_us
        .checked_mul(1_000)
        .context("durable commit duration overflows nanoseconds")?;
    Ok(ThroughputReport {
        ingestion: throughput_rates(blocks, phase.logical, ingestion_wall_ns),
        backend_commit: throughput_rates(blocks, phase.logical, backend_commit_ns),
        durable_commit: throughput_rates(blocks, phase.logical, durable_commit_ns),
    })
}

fn throughput_rates(blocks: u64, logical: LogicalVolume, elapsed_ns: u64) -> ThroughputRates {
    const BYTES_PER_MIB: f64 = 1_048_576.0;

    let seconds = elapsed_ns as f64 / 1_000_000_000.0;
    let per_second = |value: f64| (seconds > 0.0).then(|| value / seconds);
    ThroughputRates {
        elapsed_ns,
        blocks_per_second: per_second(blocks as f64),
        operations_per_second: per_second(logical.entries as f64),
        value_mib_per_second: per_second(logical.value_bytes as f64 / BYTES_PER_MIB),
        mutation_mib_per_second: per_second(logical.mutation_bytes as f64 / BYTES_PER_MIB),
    }
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn unix_time_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdbx_benchmark::{
        BenchmarkLabels, CampaignScale, MdbxBenchmarkConfig, SmokeSettings,
    };
    use crate::storage_workload::{ValueSizeClass, WorkloadShape};
    use tempfile::tempdir;

    fn tiny_shape() -> WorkloadShape {
        WorkloadShape {
            source: "mdbx-runner-test",
            seed: 99,
            prefill_rows: 24,
            blocks: 6,
            commit_count: 3,
            put_count: 10,
            tombstone_count: 2,
            version_hit_count: 4,
            value_sizes: [
                ValueSizeClass::new(6, 192),
                ValueSizeClass::new(4, 384),
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
    fn tiny_durable_campaign_reopens_and_serializes() {
        let temp = tempdir().expect("temporary root");
        let database = temp.path().join("db");
        let config = MdbxBenchmarkConfig {
            database,
            evidence_log: Some(temp.path().join("evidence.jsonl")),
            shape: tiny_shape(),
            scale: CampaignScale::Smoke,
            smoke: SmokeSettings::default(),
            prefill_batch_entries: 7,
            point_queries: 8,
            point_rounds: 2,
            sorted_batch_keys: 3,
            sorted_batch_rounds: 2,
            labels: BenchmarkLabels {
                hardware: "test-cpu".to_owned(),
                filesystem: "temporary-test-filesystem".to_owned(),
                durability: "mdbx-safe-durable".to_owned(),
                read_cache_state: "uncontrolled-warm-test".to_owned(),
            },
        };
        let first = run_mdbx_benchmark(&config).expect("run durable campaign");
        assert_eq!(first.status, "completed");
        assert_eq!(first.prefill.logical.entries, 24);
        assert_eq!(first.prefill.mdbx.committed_transactions, 4);
        assert_eq!(first.campaign.logical.entries, 12);
        assert_eq!(first.campaign.mdbx.committed_transactions, 3);
        assert_eq!(
            first.campaign.logical.value_size_counts,
            first.workload.value_size_counts
        );
        assert_eq!(first.epochs.len(), 3);
        assert_eq!(first.reopen.expected_digest, first.reopen.actual_digest);
        assert!(first.reopen.matched);
        assert!(first.reopen.prefill_keys >= 4);
        assert!(first.reopen.campaign_keys >= 4);
        assert!(first.reopen.version_hit_puts > 0);
        assert!(first.reopen.tombstones > 0);
        assert_eq!(first.reopen.epoch_coverage.len(), 3);
        assert!(
            first
                .reopen
                .epoch_coverage
                .iter()
                .all(|epoch| { epoch.new_puts + epoch.version_hit_puts + epoch.tombstones > 0 })
        );
        assert_eq!(first.reopen.absent_keys * 2, first.reopen.verified_keys);
        assert_eq!(
            first.reopen.pre_drop_transaction_id,
            first.reopen.transaction_id
        );
        assert_eq!(first.reads.point.keys, 16);
        assert_eq!(first.reads.sorted_batch.keys, 16);
        assert_eq!(first.reads.percentile_method, "nearest_rank");
        assert_eq!(first.reads.sorted_batch.target_keys_per_call, 3);
        assert_eq!(first.reads.sorted_batch.min_keys_per_call, 2);
        assert_eq!(first.reads.sorted_batch.max_keys_per_call, 3);
        assert_eq!(first.effective_mdbx_sync_mode, "durable");
        assert_eq!(first.mdbx_runtime.sync_mode, "durable");
        assert!(first.mdbx_runtime.geometry_upper_bytes > 0);
        assert_ne!(first.build.profile, "unavailable");
        assert_ne!(first.build.target, "unavailable");
        assert_eq!(
            first.campaign_throughput.ingestion.elapsed_ns,
            first
                .epochs
                .iter()
                .map(|epoch| epoch.phase.wall_ns)
                .sum::<u64>()
        );
        assert!(
            first.campaign_throughput.ingestion.elapsed_ns <= first.campaign.wall_ns,
            "throughput denominator must exclude nested instrumentation overhead"
        );
        assert!(
            first.campaign_throughput.durable_commit.elapsed_ns
                <= first.campaign_throughput.backend_commit.elapsed_ns
        );
        assert!(
            first.campaign_throughput.backend_commit.elapsed_ns
                <= first.campaign_throughput.ingestion.elapsed_ns
        );
        assert!(
            first
                .campaign_throughput
                .ingestion
                .blocks_per_second
                .is_some_and(|rate| rate > 0.0)
        );
        assert_eq!(
            first.campaign.mdbx.durable_commit_us,
            first
                .campaign
                .mdbx
                .stages
                .iter()
                .find(|stage| stage.name == "commit")
                .expect("durable commit stage")
                .total
        );
        let json = serde_json::to_value(&first).expect("serialize report");
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["backend"], "mdbx");
        assert_eq!(json["campaign"]["logical"]["value_bytes"], 576);
    }

    #[test]
    fn runner_rejects_a_nonempty_database() {
        let temp = tempdir().expect("temporary root");
        fs::write(temp.path().join("existing"), b"do not delete").expect("seed file");
        let config = MdbxBenchmarkConfig {
            database: temp.path().to_path_buf(),
            evidence_log: None,
            shape: tiny_shape(),
            scale: CampaignScale::Smoke,
            smoke: SmokeSettings::default(),
            prefill_batch_entries: 7,
            point_queries: 4,
            point_rounds: 1,
            sorted_batch_keys: 2,
            sorted_batch_rounds: 1,
            labels: BenchmarkLabels {
                hardware: "test".to_owned(),
                filesystem: "test".to_owned(),
                durability: "durable".to_owned(),
                read_cache_state: "test".to_owned(),
            },
        };
        let error = run_mdbx_benchmark(&config).expect_err("must reject nonempty database");
        assert!(error.to_string().contains("not empty"));
        assert_eq!(
            fs::read(temp.path().join("existing")).unwrap(),
            b"do not delete"
        );
    }

    #[test]
    fn operation_digest_is_stable_and_domain_separated() {
        let campaign = WorkloadCampaign::new(tiny_shape()).expect("campaign");
        let operations = campaign
            .commits()
            .flat_map(|batch| batch.operations().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let digest = |domain: &'static [u8]| {
            let mut digest = OperationDigest::new(domain);
            for operation in &operations {
                digest.observe(operation).expect("observe operation");
            }
            digest.finish()
        };
        assert_eq!(digest(b"a"), digest(b"a"));
        assert_ne!(digest(b"a"), digest(b"b"));
    }
}

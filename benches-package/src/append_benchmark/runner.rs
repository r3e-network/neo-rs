use super::store::AppendStore;
use super::{
    AppendBenchmarkConfig, AppendBenchmarkReport, AppendCompactionReport, AppendLayoutReport,
    AppendPhaseReport, AppendReadReport, AppendReadsReport, AppendReopenReport, AppendStageTotals,
    AppendThroughputReport, PercentileReport,
};
use crate::mdbx_benchmark::{
    EvidenceLog, LogicalVolume, RssSampler, WorkloadReport, capture_evidence,
    clock_ticks_per_second, evidence_delta, resolve_shape, validate_benchmark_artifact_paths,
};
use crate::storage_workload::{
    MPT_NODE_KEY_BYTES, OperationKind, VALUE_SIZE_BUCKET_UPPER_BOUNDS, WorkloadCampaign,
    WorkloadOperation,
};
use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RSS_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
static RUNNER_LEASE: Mutex<()> = Mutex::new(());

/// Runs one fresh durable append-frame campaign.
pub fn run_append_benchmark(config: &AppendBenchmarkConfig) -> Result<AppendBenchmarkReport> {
    let _lease = RUNNER_LEASE
        .lock()
        .map_err(|error| anyhow::anyhow!("append benchmark process lease is poisoned: {error}"))?;
    validate_config(config)?;
    let shape = resolve_shape(config.shape, config.scale, config.smoke)?;
    validate_sample_capacity(config.point_queries, &shape)?;
    validate_benchmark_artifact_paths(
        &config.database,
        config.evidence_log.as_deref(),
        &config.output,
    )?;

    let run_started = Instant::now();
    let clock_ticks = clock_ticks_per_second();
    let mut evidence_log = EvidenceLog::new(config.evidence_log.as_deref())?;
    let campaign = WorkloadCampaign::new(shape)
        .map_err(anyhow::Error::new)
        .context("construct resolved append workload")?;
    let mut store = AppendStore::create(&config.database, config.max_index_memory_bytes)?;

    let requested_hits = config.point_queries.div_ceil(2);
    let mut verification = VerificationCorpus::new(requested_hits)?;
    let prefill_measurement =
        PhaseMeasurement::begin("prefill", &config.database, &mut evidence_log)?;
    let mut prefill_volume = LogicalVolume::default();
    let mut prefill_digest = OperationDigest::new(b"neo-mdbx-bench-prefill-v1");
    let mut prefill_totals = AppendStageTotals::default();
    let mut prefill = campaign.prefill();
    loop {
        let operations = prefill
            .by_ref()
            .take(config.prefill_batch_entries)
            .collect::<Vec<_>>();
        if operations.is_empty() {
            break;
        }
        observe_operations(
            &operations,
            &mut prefill_volume,
            &mut prefill_digest,
            Some(&mut verification),
        )?;
        prefill_totals.merge(
            store
                .append(&operations)
                .context("append durable prefill frame")?,
        );
    }
    ensure!(
        verification.prefill_keys == verification.requested_prefill_keys,
        "prefill retained {} verification keys, expected {}",
        verification.prefill_keys,
        verification.requested_prefill_keys
    );
    ensure!(
        prefill_volume.entries == shape.prefill_rows,
        "generated prefill entry count {} does not match resolved shape {}",
        prefill_volume.entries,
        shape.prefill_rows
    );
    let prefill_report = prefill_measurement.finish(
        &config.database,
        &mut evidence_log,
        prefill_volume,
        prefill_totals,
        clock_ticks,
    )?;

    let campaign_measurement =
        PhaseMeasurement::begin("campaign", &config.database, &mut evidence_log)?;
    let mut campaign_volume = LogicalVolume::default();
    let mut campaign_digest = OperationDigest::new(b"neo-mdbx-bench-campaign-v1");
    let mut campaign_totals = AppendStageTotals::default();
    for batch in campaign.commits() {
        let operations = batch.operations().collect::<Vec<_>>();
        observe_operations(
            &operations,
            &mut campaign_volume,
            &mut campaign_digest,
            None,
        )?;
        campaign_totals.merge(
            store
                .append(&operations)
                .with_context(|| format!("append durable campaign epoch {}", batch.commit_index))?,
        );
        verification.apply_campaign_epoch(&operations)?;
    }
    let campaign_report = campaign_measurement.finish(
        &config.database,
        &mut evidence_log,
        campaign_volume,
        campaign_totals,
        clock_ticks,
    )?;
    ensure!(
        campaign_volume.entries == campaign.operation_count(),
        "generated campaign operation count does not match resolved shape"
    );
    ensure!(
        u128::from(campaign_volume.value_bytes) == campaign.logical_value_bytes(),
        "generated campaign value bytes do not match resolved shape"
    );
    // Reclaim superseded runs and manifests before the read phase, so the
    // measured reads run against the compacted, garbage-collected layout.
    store
        .gc()
        .context("reclaim superseded append index runs and manifests")?;
    let compaction_stats = store.compaction_stats();
    let reads = benchmark_reads(
        &store,
        &verification.read_queries(config.point_queries)?,
        config,
    )?;

    let reopen_queries = verification.reopen_queries()?;
    let expected_reopen_digest = verification_digest_expected(&reopen_queries)?;
    let (pack_bytes, index_bytes, run_count, decoded_index_memory_bytes) = store.layout()?;
    drop(store);

    let reopen_started = Instant::now();
    let reopened = AppendStore::open(&config.database, config.max_index_memory_bytes)
        .context("reopen append prototype")?;
    let reopen_ns = duration_ns(reopen_started.elapsed());
    let (actual_reopen_digest, verified_keys) = verify_reopened(&reopened, &reopen_queries)?;
    ensure!(
        expected_reopen_digest == actual_reopen_digest,
        "reopened append digest mismatch: expected {expected_reopen_digest}, actual {actual_reopen_digest}"
    );
    let validation = reopened.open_validation();
    let reopened_layout = reopened.layout()?;
    ensure!(
        reopened_layout
            == (
                pack_bytes,
                index_bytes,
                run_count,
                decoded_index_memory_bytes
            ),
        "append layout changed across reopen"
    );
    let final_snapshot = capture_evidence(&config.database)?;
    evidence_log.checkpoint("reopen", "verified", &final_snapshot)?;
    let campaign_throughput = throughput(shape.blocks, campaign_volume, campaign_report.wall_ns);

    let report = AppendBenchmarkReport {
        schema_version: 3,
        status: "passed".to_owned(),
        backend: "append_frames_immutable_sorted_runs_v3".to_owned(),
        output: config.output.clone(),
        database: config.database.clone(),
        evidence_log: config.evidence_log.clone(),
        scale: config.scale,
        labels: config.labels.clone(),
        workload: WorkloadReport::from(shape),
        prefill: prefill_report,
        campaign: campaign_report,
        campaign_throughput,
        reads,
        compaction: AppendCompactionReport {
            cycles: compaction_stats.cycles,
            runs_merged: compaction_stats.runs_merged,
            runs_produced: compaction_stats.runs_produced,
            input_records: compaction_stats.input_records,
            output_records: compaction_stats.output_records,
            bytes_written: compaction_stats.bytes_written,
            wall_ns: compaction_stats.wall_ns,
            peak_live_runs: compaction_stats.peak_live_runs,
            live_runs_final: run_count,
            gc_cycles: compaction_stats.gc_cycles,
            gc_runs_deleted: compaction_stats.gc_runs_deleted,
            gc_manifests_deleted: compaction_stats.gc_manifests_deleted,
            gc_bytes_reclaimed: compaction_stats.gc_bytes_reclaimed,
        },
        reopen: AppendReopenReport {
            open_ns: reopen_ns,
            verified_keys,
            expected_digest: expected_reopen_digest,
            actual_digest: actual_reopen_digest,
            matched: true,
            frames_validated: validation.frames,
            runs_validated: validation.runs,
            index_entries: validation.index_entries,
        },
        layout: AppendLayoutReport {
            pack_bytes,
            index_bytes,
            run_count,
            decoded_index_memory_bytes,
            max_index_memory_bytes: config.max_index_memory_bytes,
        },
        prefill_sha256: prefill_digest.finish(),
        campaign_sha256: campaign_digest.finish(),
        total_wall_ns: duration_ns(run_started.elapsed()),
        evidence_limitations: vec![
            "prototype compaction of derived index runs is synchronous inside the append path; manifest generations gate visibility and snapshot leases pin generations, but the MDBX high-water commit marker and node shadow integration are not implemented".to_owned(),
            "open fully re-hashes every frame and immutable index run selected by the committed manifest; this is correctness-first and can make startup expensive for a large history".to_owned(),
            "process I/O includes report checkpoints when an evidence log is configured".to_owned(),
            "resident index memory (filters, fences, and run metadata) is deliberately bounded; reaching the bound fails the campaign rather than allocating without limit".to_owned(),
        ],
    };
    write_json_report(&config.output, &report)?;
    Ok(report)
}

fn validate_config(config: &AppendBenchmarkConfig) -> Result<()> {
    for (name, value) in [
        ("hardware", config.labels.hardware.as_str()),
        ("filesystem", config.labels.filesystem.as_str()),
        ("durability", config.labels.durability.as_str()),
        ("read cache state", config.labels.read_cache_state.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{name} profile label must not be empty");
        }
    }
    ensure!(
        config.prefill_batch_entries > 0,
        "prefill batch must be non-zero"
    );
    ensure!(
        config.point_queries >= 2,
        "point query count must be at least two"
    );
    ensure!(
        config.point_rounds > 0 && config.sorted_batch_rounds > 0,
        "read benchmark rounds must be non-zero"
    );
    ensure!(
        config.sorted_batch_keys > 0,
        "sorted batch size must be non-zero"
    );
    ensure!(
        config.max_index_memory_bytes > 0,
        "index memory bound must be non-zero"
    );
    if config.scale == crate::mdbx_benchmark::CampaignScale::Full
        && cfg!(debug_assertions)
        && !cfg!(test)
    {
        bail!("full append campaigns require --release");
    }
    Ok(())
}

fn validate_sample_capacity(
    point_queries: usize,
    shape: &crate::storage_workload::WorkloadShape,
) -> Result<()> {
    let required =
        u64::try_from(point_queries.div_ceil(2)).context("point sample count does not fit u64")?;
    ensure!(
        required <= shape.prefill_rows,
        "point reads require {required} prefill rows, resolved workload has {}",
        shape.prefill_rows
    );
    let version_hit_puts = shape
        .version_hit_count
        .checked_sub(shape.tombstone_count)
        .context("resolved version-hit put count underflows")?;
    let new_puts = shape
        .put_count
        .checked_sub(version_hit_puts)
        .context("resolved new-put count underflows")?;
    ensure!(
        required <= new_puts,
        "point reads require {required} new campaign rows, resolved workload has {new_puts}"
    );
    Ok(())
}

struct PhaseMeasurement {
    name: String,
    database: PathBuf,
    before: crate::mdbx_benchmark::EvidenceSnapshot,
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
            started: Instant::now(),
            rss: RssSampler::start(RSS_SAMPLE_INTERVAL),
        })
    }

    fn finish(
        self,
        database: &Path,
        evidence_log: &mut EvidenceLog,
        logical: LogicalVolume,
        stages: AppendStageTotals,
        clock_ticks: Option<u64>,
    ) -> Result<AppendPhaseReport> {
        ensure!(self.database == database, "phase database path changed");
        let wall = self.started.elapsed();
        let after = capture_evidence(database)?;
        let sampled_rss = self.rss.finish();
        evidence_log.checkpoint(&self.name, "after", &after)?;
        let evidence_delta = evidence_delta(&self.before, &after, wall, clock_ticks);
        let write_bytes = evidence_delta.process.write_bytes;
        Ok(AppendPhaseReport {
            name: self.name,
            wall_ns: duration_ns(wall),
            logical,
            before: self.before,
            after,
            evidence_delta,
            sampled_rss,
            process_physical_write_bytes: write_bytes,
            write_amplification_vs_values: ratio(write_bytes, logical.value_bytes),
            write_amplification_vs_mutations: ratio(write_bytes, logical.mutation_bytes),
            append_write_ns: stages.append_write_ns,
            pack_sync_ns: stages.pack_sync_ns,
            index_build_ns: stages.index_build_ns,
            publication_overlap_ns: stages.publication_overlap_ns,
            index_write_ns: stages.index_write_ns,
            index_sync_ns: stages.index_sync_ns,
            directory_sync_ns: stages.directory_sync_ns,
            frames: stages.frames,
            index_entries: stages.index_entries,
        })
    }
}

fn observe_operations(
    operations: &[WorkloadOperation],
    volume: &mut LogicalVolume,
    digest: &mut OperationDigest,
    mut verification: Option<&mut VerificationCorpus>,
) -> Result<()> {
    for operation in operations {
        volume.merge(observe_operation(operation)?)?;
        digest.observe(operation)?;
        if let Some(verification) = verification.as_deref_mut() {
            verification.capture_prefill(operation)?;
        }
    }
    Ok(())
}

fn observe_operation(operation: &WorkloadOperation) -> Result<LogicalVolume> {
    let key_bytes = u64::try_from(operation.key.len()).context("key length does not fit u64")?;
    Ok(match &operation.kind {
        OperationKind::Put(value) => {
            let value_bytes =
                u64::try_from(value.len()).context("value length does not fit u64")?;
            let bucket = VALUE_SIZE_BUCKET_UPPER_BOUNDS
                .iter()
                .position(|upper| upper.is_none_or(|upper| value.len() <= upper))
                .context("value does not fit a workload bucket")?;
            let mut counts = [0u64; 8];
            counts[bucket] = 1;
            LogicalVolume {
                puts: 1,
                tombstones: 0,
                entries: 1,
                key_bytes,
                value_bytes,
                mutation_bytes: key_bytes
                    .checked_add(value_bytes)
                    .context("mutation byte count overflows")?,
                value_size_counts: counts,
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
                self.0.update(
                    &u64::try_from(value.len())
                        .context("digest value length does not fit u64")?
                        .to_le_bytes(),
                );
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum MutationKind {
    NewPut,
    VersionHitPut,
    Tombstone,
}

#[derive(Clone)]
struct VerificationSample {
    key: [u8; MPT_NODE_KEY_BYTES],
    expected: Option<Vec<u8>>,
    from_prefill: bool,
    new_campaign_put: bool,
}

struct VerificationCorpus {
    requested_prefill_keys: usize,
    requested_campaign_keys: usize,
    prefill_keys: usize,
    campaign_keys: usize,
    samples: Vec<VerificationSample>,
    indices: HashMap<[u8; MPT_NODE_KEY_BYTES], usize>,
}

impl VerificationCorpus {
    fn new(requested_hits: usize) -> Result<Self> {
        let capacity = requested_hits
            .checked_mul(2)
            .context("verification capacity overflows")?;
        Ok(Self {
            requested_prefill_keys: requested_hits,
            requested_campaign_keys: requested_hits,
            prefill_keys: 0,
            campaign_keys: 0,
            samples: Vec::with_capacity(capacity),
            indices: HashMap::with_capacity(capacity),
        })
    }

    fn capture_prefill(&mut self, operation: &WorkloadOperation) -> Result<()> {
        if self.prefill_keys >= self.requested_prefill_keys {
            return Ok(());
        }
        let OperationKind::Put(value) = &operation.kind else {
            bail!("prefill emitted a tombstone");
        };
        let index = self.samples.len();
        ensure!(
            self.indices.insert(operation.key, index).is_none(),
            "prefill generated a duplicate sampled key"
        );
        self.samples.push(VerificationSample {
            key: operation.key,
            expected: Some(value.clone()),
            from_prefill: true,
            new_campaign_put: false,
        });
        self.prefill_keys += 1;
        Ok(())
    }

    fn apply_campaign_epoch(&mut self, operations: &[WorkloadOperation]) -> Result<()> {
        for kind in [
            MutationKind::NewPut,
            MutationKind::VersionHitPut,
            MutationKind::Tombstone,
        ] {
            if let Some(operation) = operations
                .iter()
                .find(|operation| mutation_kind(operation) == kind)
            {
                self.ensure_campaign_sample(kind, operation);
            }
        }
        if self.campaign_keys < self.requested_campaign_keys {
            for operation in operations
                .iter()
                .filter(|operation| mutation_kind(operation) == MutationKind::NewPut)
            {
                self.ensure_campaign_sample(MutationKind::NewPut, operation);
                if self.campaign_keys >= self.requested_campaign_keys {
                    break;
                }
            }
        }
        for operation in operations {
            if let Some(index) = self.indices.get(&operation.key).copied() {
                self.samples[index].expected = operation_value(operation);
            }
        }
        Ok(())
    }

    fn ensure_campaign_sample(&mut self, kind: MutationKind, operation: &WorkloadOperation) {
        let index = self
            .indices
            .get(&operation.key)
            .copied()
            .unwrap_or_else(|| {
                let index = self.samples.len();
                self.indices.insert(operation.key, index);
                self.samples.push(VerificationSample {
                    key: operation.key,
                    expected: operation_value(operation),
                    from_prefill: false,
                    new_campaign_put: false,
                });
                index
            });
        if kind == MutationKind::NewPut && !self.samples[index].new_campaign_put {
            self.samples[index].new_campaign_put = true;
            self.campaign_keys += 1;
        }
    }

    fn read_queries(&self, count: usize) -> Result<Vec<VerificationSample>> {
        let hit_count = count.div_ceil(2);
        let prefill_target = hit_count / 2;
        let campaign_target = hit_count - prefill_target;
        let mut selected = Vec::with_capacity(hit_count);
        let mut selected_indices = HashSet::with_capacity(hit_count);
        self.select(
            &mut selected,
            &mut selected_indices,
            prefill_target,
            |sample| sample.from_prefill,
        );
        self.select(
            &mut selected,
            &mut selected_indices,
            campaign_target,
            |sample| sample.new_campaign_put,
        );
        if selected.len() < hit_count {
            let remaining = hit_count - selected.len();
            self.select(&mut selected, &mut selected_indices, remaining, |_| true);
        }
        ensure!(
            selected.len() == hit_count,
            "insufficient present read samples"
        );
        let mut queries = Vec::with_capacity(count);
        for index in selected {
            let hit = &self.samples[index];
            queries.push(hit.clone());
            if queries.len() == count {
                break;
            }
            queries.push(missing_sample(hit));
            if queries.len() == count {
                break;
            }
        }
        ensure!(queries.len() == count, "failed to build exact read corpus");
        Ok(queries)
    }

    fn select(
        &self,
        selected: &mut Vec<usize>,
        selected_indices: &mut HashSet<usize>,
        count: usize,
        predicate: impl Fn(&VerificationSample) -> bool,
    ) {
        let target = selected.len().saturating_add(count);
        for (index, sample) in self.samples.iter().enumerate() {
            if selected.len() >= target {
                break;
            }
            if sample.expected.is_some() && predicate(sample) && selected_indices.insert(index) {
                selected.push(index);
            }
        }
    }

    fn reopen_queries(&self) -> Result<Vec<VerificationSample>> {
        let mut queries = Vec::with_capacity(
            self.samples
                .len()
                .checked_mul(2)
                .context("reopen corpus capacity overflows")?,
        );
        for sample in &self.samples {
            queries.push(sample.clone());
            queries.push(missing_sample(sample));
        }
        Ok(queries)
    }
}

fn mutation_kind(operation: &WorkloadOperation) -> MutationKind {
    match (&operation.kind, operation.version_hit) {
        (OperationKind::Tombstone, _) => MutationKind::Tombstone,
        (OperationKind::Put(_), true) => MutationKind::VersionHitPut,
        (OperationKind::Put(_), false) => MutationKind::NewPut,
    }
}

fn operation_value(operation: &WorkloadOperation) -> Option<Vec<u8>> {
    match &operation.kind {
        OperationKind::Put(value) => Some(value.clone()),
        OperationKind::Tombstone => None,
    }
}

fn missing_sample(sample: &VerificationSample) -> VerificationSample {
    let mut key = sample.key;
    key[0] = key[0].wrapping_add(1);
    VerificationSample {
        key,
        expected: None,
        from_prefill: false,
        new_campaign_put: false,
    }
}

fn benchmark_reads(
    store: &AppendStore,
    queries: &[VerificationSample],
    config: &AppendBenchmarkConfig,
) -> Result<AppendReadsReport> {
    let mut point_calls = Vec::new();
    let mut point_bytes = 0u64;
    for _ in 0..config.point_rounds {
        for query in queries {
            let started = Instant::now();
            let actual = store.get(&query.key)?;
            point_calls.push(duration_ns(started.elapsed()));
            ensure!(actual == query.expected, "append point read mismatch");
            point_bytes = point_bytes
                .checked_add(actual.as_ref().map_or(0, |value| value.len() as u64))
                .context("point read bytes overflow")?;
            std::hint::black_box(actual);
        }
    }

    let mut sorted = queries.to_vec();
    sorted.sort_unstable_by_key(|sample| sample.key);
    let mut batch_calls = Vec::new();
    let mut batch_per_key = Vec::new();
    let mut batch_bytes = 0u64;
    for _ in 0..config.sorted_batch_rounds {
        for chunk in sorted.chunks(config.sorted_batch_keys) {
            let keys = chunk.iter().map(|sample| sample.key).collect::<Vec<_>>();
            let started = Instant::now();
            let actual = store.get_many_sorted(&keys)?;
            let elapsed = duration_ns(started.elapsed());
            batch_calls.push(elapsed);
            let chunk_len = u64::try_from(chunk.len()).context("empty read chunk")?;
            ensure!(
                actual.len() == chunk.len(),
                "append sorted lookup changed result count"
            );
            batch_per_key.push(elapsed / chunk_len);
            for (actual, expected) in actual.iter().zip(chunk) {
                ensure!(actual == &expected.expected, "append sorted read mismatch");
                batch_bytes = batch_bytes
                    .checked_add(actual.as_ref().map_or(0, |value| value.len() as u64))
                    .context("batch read bytes overflow")?;
            }
            std::hint::black_box(actual);
        }
    }
    let keys = u64::try_from(queries.len()).context("query count does not fit u64")?;
    let hits = u64::try_from(
        queries
            .iter()
            .filter(|query| query.expected.is_some())
            .count(),
    )
    .context("hit count does not fit u64")?;
    let misses = keys
        .checked_sub(hits)
        .context("hit count exceeds query count")?;
    Ok(AppendReadsReport {
        percentile_method: "nearest_rank".to_owned(),
        point: AppendReadReport {
            mode: "point".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.point_rounds,
            keys: keys.saturating_mul(u64::from(config.point_rounds)),
            hits: hits.saturating_mul(u64::from(config.point_rounds)),
            misses: misses.saturating_mul(u64::from(config.point_rounds)),
            value_bytes: point_bytes,
            target_keys_per_call: 1,
            call_latency: percentile(&point_calls),
            normalized_per_key_latency: percentile(&point_calls),
        },
        sorted_batch: AppendReadReport {
            mode: "sorted_batch".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.sorted_batch_rounds,
            keys: keys.saturating_mul(u64::from(config.sorted_batch_rounds)),
            hits: hits.saturating_mul(u64::from(config.sorted_batch_rounds)),
            misses: misses.saturating_mul(u64::from(config.sorted_batch_rounds)),
            value_bytes: batch_bytes,
            target_keys_per_call: config.sorted_batch_keys,
            call_latency: percentile(&batch_calls),
            normalized_per_key_latency: percentile(&batch_per_key),
        },
    })
}

fn verify_reopened(store: &AppendStore, queries: &[VerificationSample]) -> Result<(String, u64)> {
    let mut digest = Sha256Hasher::new();
    digest.update(b"neo-mdbx-bench-reopen-v1");
    for query in queries {
        let actual = store.get(&query.key)?;
        ensure!(actual == query.expected, "reopened append value mismatch");
        digest_value(&mut digest, &query.key, actual.as_deref())?;
    }
    Ok((
        hex::encode(digest.finalize()),
        u64::try_from(queries.len()).context("reopen count does not fit u64")?,
    ))
}

fn verification_digest_expected(queries: &[VerificationSample]) -> Result<String> {
    let mut digest = Sha256Hasher::new();
    digest.update(b"neo-mdbx-bench-reopen-v1");
    for query in queries {
        digest_value(&mut digest, &query.key, query.expected.as_deref())?;
    }
    Ok(hex::encode(digest.finalize()))
}

fn digest_value(
    digest: &mut Sha256Hasher,
    key: &[u8; MPT_NODE_KEY_BYTES],
    value: Option<&[u8]>,
) -> Result<()> {
    digest.update(key);
    match value {
        Some(value) => {
            digest.update(&[1]);
            digest.update(
                &u64::try_from(value.len())
                    .context("verification value length does not fit u64")?
                    .to_le_bytes(),
            );
            digest.update(value);
        }
        None => digest.update(&[0]),
    }
    Ok(())
}

fn throughput(blocks: u64, logical: LogicalVolume, elapsed_ns: u64) -> AppendThroughputReport {
    let seconds = elapsed_ns as f64 / 1_000_000_000.0;
    let per_second = |value: f64| if seconds > 0.0 { value / seconds } else { 0.0 };
    AppendThroughputReport {
        blocks_per_second: per_second(blocks as f64),
        operations_per_second: per_second(logical.entries as f64),
        value_mib_per_second: per_second(logical.value_bytes as f64 / 1_048_576.0),
        mutation_mib_per_second: per_second(logical.mutation_bytes as f64 / 1_048_576.0),
    }
}

fn percentile(values: &[u64]) -> PercentileReport {
    if values.is_empty() {
        return PercentileReport::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = |percent: usize| {
        let index = (percent * sorted.len()).div_ceil(100).max(1) - 1;
        sorted[index]
    };
    let sum = sorted
        .iter()
        .fold(0u128, |total, value| total + u128::from(*value));
    PercentileReport {
        samples: sorted.len() as u64,
        min_ns: sorted[0],
        mean_ns: (sum / sorted.len() as u128) as u64,
        p50_ns: rank(50),
        p95_ns: rank(95),
        p99_ns: rank(99),
        max_ns: sorted[sorted.len() - 1],
    }
}

fn ratio(numerator: Option<u64>, denominator: u64) -> Option<f64> {
    numerator.and_then(|value| (denominator > 0).then(|| value as f64 / denominator as f64))
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn write_json_report(path: &Path, report: &AppendBenchmarkReport) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create report directory {}", parent.display()))?;
    let name = path
        .file_name()
        .context("append report path has no file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = File::create(&temporary)
            .with_context(|| format!("create temporary report {}", temporary.display()))?;
        serde_json::to_writer_pretty(&mut file, report).context("encode append report")?;
        file.write_all(b"\n").context("terminate append report")?;
        file.sync_all().context("sync append report")?;
        fs::rename(&temporary, path).context("publish append report")?;
        File::open(parent)
            .context("open append report directory")?
            .sync_all()
            .context("sync append report directory")
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdbx_benchmark::{BenchmarkLabels, CampaignScale, SmokeSettings};
    use crate::storage_workload::{ValueSizeClass, WorkloadShape};
    use tempfile::tempdir;

    #[test]
    fn tiny_campaign_reopens_and_publishes_report() {
        let root = tempdir().expect("temporary root");
        let database = root.path().join("append-db");
        let output = root.path().join("append.json");
        let shape = WorkloadShape {
            source: "append-runner-test",
            seed: 17,
            prefill_rows: 32,
            blocks: 4,
            commit_count: 2,
            put_count: 16,
            tombstone_count: 2,
            version_hit_count: 4,
            value_sizes: [
                ValueSizeClass::new(14, 448),
                ValueSizeClass::new(2, 160),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
                ValueSizeClass::new(0, 0),
            ],
        };
        let report = run_append_benchmark(&AppendBenchmarkConfig {
            database,
            output: output.clone(),
            evidence_log: None,
            shape,
            scale: CampaignScale::Smoke,
            smoke: SmokeSettings {
                prefill_rows: 32,
                operations: 18,
                blocks: 4,
            },
            prefill_batch_entries: 8,
            point_queries: 8,
            point_rounds: 1,
            sorted_batch_keys: 4,
            sorted_batch_rounds: 1,
            max_index_memory_bytes: 1024 * 1024,
            labels: BenchmarkLabels {
                hardware: "test".to_owned(),
                filesystem: "temp".to_owned(),
                durability: "sync-data".to_owned(),
                read_cache_state: "warm".to_owned(),
            },
        })
        .expect("run append benchmark");
        assert!(report.reopen.matched);
        assert_eq!(report.reopen.expected_digest, report.reopen.actual_digest);
        assert_eq!(report.campaign.logical.entries, 18);
        assert!(output.is_file());
    }
}

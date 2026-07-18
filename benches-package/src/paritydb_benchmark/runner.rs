use super::store::ParityDbStore;
use super::{
    ParityDbBenchmarkConfig, ParityDbBenchmarkReport, ParityDbLayoutReport,
    ParityDbMaintenanceReport, ParityDbPhaseReport, ParityDbReopenReport, ParityDbStageTotals,
    ParityDbThroughputReport,
};
use crate::mdbx_benchmark::{
    DigestReport, EvidenceLog, LogicalVolume, ReadBenchmarksReport, ReadLatencyReport, RssSampler,
    WorkloadReport, capture_evidence, clock_ticks_per_second, evidence_delta, percentile_report,
    resolve_shape, validate_benchmark_artifact_paths,
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

/// Runs one fresh, explicitly fenced ParityDB hash-column campaign.
pub fn run_paritydb_benchmark(config: &ParityDbBenchmarkConfig) -> Result<ParityDbBenchmarkReport> {
    let _lease = RUNNER_LEASE
        .lock()
        .map_err(|error| anyhow::anyhow!("ParityDB benchmark lease is poisoned: {error}"))?;
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
        .context("construct resolved ParityDB workload")?;
    let mut store = ParityDbStore::create(&config.database)?;

    let requested_hits = config.point_queries.div_ceil(2);
    let mut verification = VerificationCorpus::new(requested_hits)?;
    let prefill_measurement =
        PhaseMeasurement::begin("prefill", &config.database, &mut evidence_log)?;
    let mut prefill_volume = LogicalVolume::default();
    let mut prefill_digest = OperationDigest::new(b"neo-mdbx-bench-prefill-v1");
    let mut prefill_stages = ParityDbStageTotals::default();
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
        prefill_stages.merge(
            store
                .commit_durable(&operations)
                .context("commit durable ParityDB prefill batch")?,
        )?;
    }
    ensure!(
        verification.prefill_keys == verification.requested_prefill_keys,
        "prefill retained {} verification keys, expected {}",
        verification.prefill_keys,
        verification.requested_prefill_keys
    );
    let prefill_report = prefill_measurement.finish(
        &config.database,
        &mut evidence_log,
        prefill_volume,
        prefill_stages,
        clock_ticks,
    )?;

    let campaign_measurement =
        PhaseMeasurement::begin("campaign", &config.database, &mut evidence_log)?;
    let mut campaign_volume = LogicalVolume::default();
    let mut campaign_digest = OperationDigest::new(b"neo-mdbx-bench-campaign-v1");
    let mut campaign_stages = ParityDbStageTotals::default();
    for batch in campaign.commits() {
        let operations = batch.operations().collect::<Vec<_>>();
        observe_operations(
            &operations,
            &mut campaign_volume,
            &mut campaign_digest,
            None,
        )?;
        campaign_stages.merge(store.commit_durable(&operations).with_context(|| {
            format!(
                "commit durable ParityDB campaign epoch {}",
                batch.commit_index
            )
        })?)?;
        verification.apply_campaign_epoch(&operations)?;
    }
    ensure!(
        campaign_volume.entries == campaign.operation_count(),
        "generated ParityDB campaign operation count differs from resolved workload"
    );
    ensure!(
        u128::from(campaign_volume.value_bytes) == campaign.logical_value_bytes(),
        "generated ParityDB campaign value bytes differ from resolved workload"
    );
    ensure!(
        verification.campaign_keys >= verification.requested_campaign_keys,
        "campaign retained {} new verification keys, expected at least {}",
        verification.campaign_keys,
        verification.requested_campaign_keys
    );
    let campaign_report = campaign_measurement.finish(
        &config.database,
        &mut evidence_log,
        campaign_volume,
        campaign_stages,
        clock_ticks,
    )?;

    let read_queries = verification.read_queries(config.point_queries)?;
    let reads = benchmark_reads(&store, &read_queries, config)?;
    let reopen_queries = verification.reopen_queries()?;
    let expected_reopen_digest = verification_digest_expected(&reopen_queries)?;
    let pre_drop_entries = store.value_entries()?;
    drop(store);

    let reopen_started = Instant::now();
    let reopened = ParityDbStore::open_read_only(&config.database)?;
    let reopen_ns = duration_ns(reopen_started.elapsed());
    let (actual_reopen_digest, verified_keys) = verify_reopened(&reopened, &reopen_queries)?;
    ensure!(
        expected_reopen_digest == actual_reopen_digest,
        "reopened ParityDB digest mismatch: expected {expected_reopen_digest}, actual {actual_reopen_digest}"
    );
    let reopened_entries = reopened.value_entries()?;
    ensure!(
        reopened_entries == pre_drop_entries,
        "ParityDB optional value entry count changed across reopen: {pre_drop_entries:?} -> {reopened_entries:?}"
    );
    let hash_index_files = reopened.hash_index_files()?;
    let hash_index_generations = u64::try_from(hash_index_files.len())
        .context("ParityDB hash-index generation count does not fit u64")?;
    let final_snapshot = capture_evidence(&config.database)?;
    evidence_log.checkpoint("reopen", "verified", &final_snapshot)?;

    let campaign_throughput = throughput(shape.blocks, campaign_volume, campaign_report.wall_ns);
    let report = ParityDbBenchmarkReport {
        schema_version: 1,
        status: "passed".to_owned(),
        backend: "parity-db-0.5.5-hash-uniform".to_owned(),
        output: config.output.clone(),
        database: config.database.clone(),
        evidence_log: config.evidence_log.clone(),
        scale: config.scale,
        labels: config.labels.clone(),
        workload: WorkloadReport::from(shape),
        configuration: ParityDbStore::configuration(),
        prefill: prefill_report,
        campaign: campaign_report,
        campaign_throughput,
        reads,
        maintenance: ParityDbMaintenanceReport {
            explicit_compaction_api: false,
            reindex_metrics_available: false,
            hash_index_generations,
            pending_reindex_inferred: hash_index_generations > 1,
            completion_semantics: "Db::drop drains queued user commits and WALs, but the 0.5.5 log-worker shutdown condition does not guarantee pending hash-index reindex completion; multiple index_00_* generations are reported as unsettled debt".to_owned(),
        },
        reopen: ParityDbReopenReport {
            open_ns: reopen_ns,
            verified_keys,
            expected_digest: expected_reopen_digest,
            actual_digest: actual_reopen_digest,
            matched: true,
            value_entries: reopened_entries,
        },
        layout: ParityDbLayoutReport {
            regular_files: final_snapshot.files.regular_files,
            logical_bytes: final_snapshot.files.logical_bytes,
            allocated_bytes: final_snapshot.files.allocated_bytes,
            value_entries: reopened_entries,
            hash_index_files,
        },
        digests: DigestReport {
            prefill_sha256: prefill_digest.finish(),
            campaign_sha256: campaign_digest.finish(),
        },
        total_wall_ns: duration_ns(run_started.elapsed()),
        evidence_limitations: vec![
            "ParityDB Db::commit is asynchronous and exposes no supported commit ticket or flush method; every benchmark transaction therefore includes Db::drop, queued-commit/WAL drain, writer reopen, and an exact transaction-sentinel read".to_owned(),
            "Db::drop reports shutdown errors through logging rather than Result; successful reopen plus sentinel equality is the observable fence check, but this is still not a production-quality per-commit durability receipt".to_owned(),
            "sync_wal=true syncs WAL files in the background pipeline and sync_data=true flushes mmap-backed index/value tables before WAL cleanup; their individual durations are not exposed by the supported API".to_owned(),
            "ParityDB database creation and metadata APIs do not expose an explicit parent-directory fsync, so the campaign does not claim crash-safe durability of newly created directory entries".to_owned(),
            "ParityDB 0.5.5 exposes incremental background hash-index reindexing but no explicit compaction call or reindex metrics; shutdown can leave reindex unfinished, so index_00_* generation count is reported as outstanding-debt evidence rather than claiming settled compaction".to_owned(),
            "get_num_column_value_entries can report that a count is unavailable for a value table; layout records null in that case and reopen correctness remains the exact sampled-value digest".to_owned(),
            "ParityDB exposes point get but no sorted multi-get for hash columns; sorted-batch latency measures one adapter call that performs ordered point gets sequentially".to_owned(),
            "the dedicated column makes the 0xf0 namespace implicit and stores only the uniformly distributed 32-byte node hash; logical workload and digest accounting retain the exact 33-byte Neo key".to_owned(),
            "process /proc/self/io write_bytes excludes filesystem journal and device-internal traffic; retained allocated bytes are footprint, not rewrite traffic".to_owned(),
            "process I/O includes optional evidence-log checkpoints and the atomically synced final JSON report is outside the measured phase".to_owned(),
        ],
    };
    write_json_report(&config.output, &report)?;
    Ok(report)
}

fn validate_config(config: &ParityDbBenchmarkConfig) -> Result<()> {
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
    if config.scale == crate::mdbx_benchmark::CampaignScale::Full
        && cfg!(debug_assertions)
        && !cfg!(test)
    {
        bail!("full ParityDB campaigns require --release");
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
        stages: ParityDbStageTotals,
        clock_ticks: Option<u64>,
    ) -> Result<ParityDbPhaseReport> {
        ensure!(self.database == database, "phase database path changed");
        let wall = self.started.elapsed();
        let after = capture_evidence(database)?;
        let sampled_rss = self.rss.finish();
        evidence_log.checkpoint(&self.name, "after", &after)?;
        let evidence_delta = evidence_delta(&self.before, &after, wall, clock_ticks);
        let write_bytes = evidence_delta.process.write_bytes;
        Ok(ParityDbPhaseReport {
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
            stages,
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
        ensure!(selected.len() == hit_count, "insufficient present samples");
        let mut queries = Vec::with_capacity(count);
        for index in selected {
            let hit = self.samples[index].clone();
            queries.push(hit.clone());
            if queries.len() == count {
                break;
            }
            queries.push(missing_sample(&hit));
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
    store: &ParityDbStore,
    queries: &[VerificationSample],
    config: &ParityDbBenchmarkConfig,
) -> Result<ReadBenchmarksReport> {
    let mut point_calls = Vec::new();
    let mut point_bytes = 0u64;
    for _ in 0..config.point_rounds {
        for query in queries {
            let started = Instant::now();
            let actual = store.get(&query.key)?;
            point_calls.push(duration_ns(started.elapsed()));
            ensure!(actual == query.expected, "ParityDB point read mismatch");
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
    let mut min_batch = usize::MAX;
    let mut max_batch = 0usize;
    for _ in 0..config.sorted_batch_rounds {
        for chunk in sorted.chunks(config.sorted_batch_keys) {
            let keys = chunk.iter().map(|sample| sample.key).collect::<Vec<_>>();
            let started = Instant::now();
            let actual = store.get_many_sorted(&keys)?;
            let elapsed = duration_ns(started.elapsed());
            let key_count = u64::try_from(chunk.len()).context("empty read chunk")?;
            batch_calls.push(elapsed);
            batch_per_key.push(elapsed / key_count);
            min_batch = min_batch.min(chunk.len());
            max_batch = max_batch.max(chunk.len());
            for (actual, expected) in actual.iter().zip(chunk) {
                ensure!(
                    actual == &expected.expected,
                    "ParityDB sorted read mismatch"
                );
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
    let misses = keys.checked_sub(hits).context("hit count exceeds keys")?;
    Ok(ReadBenchmarksReport {
        percentile_method: "nearest_rank".to_owned(),
        point: ReadLatencyReport {
            mode: "point".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.point_rounds,
            keys: keys.saturating_mul(u64::from(config.point_rounds)),
            hits: hits.saturating_mul(u64::from(config.point_rounds)),
            misses: misses.saturating_mul(u64::from(config.point_rounds)),
            value_bytes: point_bytes,
            target_keys_per_call: 1,
            min_keys_per_call: 1,
            max_keys_per_call: 1,
            call_latency: percentile_report(&point_calls),
            normalized_per_key_latency: percentile_report(&point_calls),
        },
        sorted_batch: ReadLatencyReport {
            mode: "sorted_hash_point_loop".to_owned(),
            cache_state: config.labels.read_cache_state.clone(),
            rounds: config.sorted_batch_rounds,
            keys: keys.saturating_mul(u64::from(config.sorted_batch_rounds)),
            hits: hits.saturating_mul(u64::from(config.sorted_batch_rounds)),
            misses: misses.saturating_mul(u64::from(config.sorted_batch_rounds)),
            value_bytes: batch_bytes,
            target_keys_per_call: config.sorted_batch_keys,
            min_keys_per_call: min_batch,
            max_keys_per_call: max_batch,
            call_latency: percentile_report(&batch_calls),
            normalized_per_key_latency: percentile_report(&batch_per_key),
        },
    })
}

fn verify_reopened(store: &ParityDbStore, queries: &[VerificationSample]) -> Result<(String, u64)> {
    let mut digest = Sha256Hasher::new();
    digest.update(b"neo-mdbx-bench-reopen-v1");
    for query in queries {
        let actual = store.get(&query.key)?;
        ensure!(actual == query.expected, "reopened ParityDB value mismatch");
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

fn throughput(blocks: u64, logical: LogicalVolume, elapsed_ns: u64) -> ParityDbThroughputReport {
    let seconds = elapsed_ns as f64 / 1_000_000_000.0;
    let per_second = |value: f64| if seconds > 0.0 { value / seconds } else { 0.0 };
    ParityDbThroughputReport {
        blocks_per_second: per_second(blocks as f64),
        operations_per_second: per_second(logical.entries as f64),
        value_mib_per_second: per_second(logical.value_bytes as f64 / 1_048_576.0),
        mutation_mib_per_second: per_second(logical.mutation_bytes as f64 / 1_048_576.0),
    }
}

fn ratio(numerator: Option<u64>, denominator: u64) -> Option<f64> {
    numerator.and_then(|value| (denominator > 0).then(|| value as f64 / denominator as f64))
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn write_json_report(path: &Path, report: &ParityDbBenchmarkReport) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create report directory {}", parent.display()))?;
    let name = path
        .file_name()
        .context("ParityDB report path has no file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = File::create(&temporary)
            .with_context(|| format!("create temporary report {}", temporary.display()))?;
        serde_json::to_writer_pretty(&mut file, report).context("encode ParityDB report")?;
        file.write_all(b"\n").context("terminate ParityDB report")?;
        file.sync_all().context("sync ParityDB report")?;
        fs::rename(&temporary, path).context("publish ParityDB report")?;
        File::open(parent)
            .context("open ParityDB report directory")?
            .sync_all()
            .context("sync ParityDB report directory")
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
    use crate::storage_workload::MAINNET_H1_877_001_TO_H1_887_000;
    use tempfile::tempdir;

    #[test]
    fn tiny_campaign_reopens_and_publishes_comparable_evidence() {
        let root = tempdir().expect("temporary root");
        let database = root.path().join("paritydb");
        let output = root.path().join("paritydb.json");
        let report = run_paritydb_benchmark(&ParityDbBenchmarkConfig {
            database,
            output: output.clone(),
            evidence_log: None,
            shape: MAINNET_H1_877_001_TO_H1_887_000,
            scale: CampaignScale::Smoke,
            smoke: SmokeSettings {
                prefill_rows: 64,
                operations: 32,
                blocks: 4,
            },
            prefill_batch_entries: 8,
            point_queries: 8,
            point_rounds: 1,
            sorted_batch_keys: 4,
            sorted_batch_rounds: 1,
            labels: BenchmarkLabels {
                hardware: "test".to_owned(),
                filesystem: "temp".to_owned(),
                durability: "paritydb-close-reopen".to_owned(),
                read_cache_state: "warm".to_owned(),
            },
        })
        .expect("run ParityDB benchmark");
        assert!(report.reopen.matched);
        assert_eq!(report.reopen.expected_digest, report.reopen.actual_digest);
        assert_eq!(
            report.reopen.expected_digest,
            "4016b3869d82d6d25bcb81931cbdba5e9d724dfa81c9dcc6ea0bbfeaebfa9c01"
        );
        assert_eq!(report.campaign.logical.entries, 32);
        assert_eq!(report.campaign.stages.durable_fences, 4);
        assert!(report.campaign.stages.close_drain_ns > 0);
        assert!(report.campaign.stages.reopen_ns > 0);
        assert!(report.configuration.uniform);
        assert_eq!(report.configuration.stored_key_bytes, 32);
        assert!(report.maintenance.hash_index_generations >= 1);
        assert_eq!(
            report.maintenance.pending_reindex_inferred,
            report.layout.hash_index_files.len() > 1
        );
        assert!(output.is_file());
        assert_eq!(
            report.digests.prefill_sha256,
            "c107a8cfcdc356aece63ceceb87d5b023b00da163ae5c1fb93db774ca7d40e9a"
        );
        assert_eq!(
            report.digests.campaign_sha256,
            "e344599b88455d2507e7835f9d45312d717c378fd1911870a736a6ebba6756b4"
        );
    }
}

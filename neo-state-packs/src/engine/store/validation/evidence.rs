use super::*;
use std::collections::HashMap;

const LOOKUP_EVIDENCE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/materialized-lookup-evidence/v4\0";
const SAMPLE_KEYSET_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/materialized-lookup-keyset/v2\0";
const FRAME_REFERENCE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/materialized-frame-reference/v2\0";
const SYNTHETIC_MISS_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/materialized-synthetic-miss/v2\0";
/// Maximum number of keys passed to one sorted lookup call.
const LOOKUP_BATCH_MAX_ENTRIES: usize = 1_024;
/// Maximum cumulative value bytes requested by one sorted lookup call.
///
/// Index entries carry a `u32` value length and the frame format permits much
/// larger values. Batching by entries alone would therefore allow one
/// malformed or unusually large sample to allocate an unbounded result.
const LOOKUP_BATCH_MAX_VALUE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_POINT_CROSS_CHECKS: usize = 4_096;
const MAX_PUBLIC_LOOKUP_KEYS: usize = 4_096;
const MAX_FRAME_REFERENCE_KEYS: usize = 100_000;
const MAX_LOOKUP_EVIDENCE_SAMPLES: usize = 1_000_000;
const SYNTHETIC_MISS_PROBES: usize = 256;
const SAMPLE_SEED: u64 = 0xD6E8_FEB8_6659_FD93;
pub(super) const FRAME_REFERENCE_VALUE_HASH_CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// Deterministic evidence for the materialized newest-version view of one
/// pinned manifest generation.
///
/// The complete winner-record digest covers every key, sequence, payload
/// location, value length, and tombstone without loading records into memory.
/// The lookup digest covers a bounded deterministic key sample and compares
/// the public lookup paths with both winner offsets and committed frame rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackMaterializedViewEvidence {
    /// Manifest generation pinned while producing this evidence.
    pub generation: u64,
    /// Immutable runs referenced by the pinned generation.
    pub live_runs: u64,
    /// Input records merge-walked across all live runs.
    pub source_records: u64,
    /// Canonical frame epoch bound to the evidence.
    pub tip_epoch: u64,
    /// Segment containing the canonical frame bound to the evidence.
    pub tip_segment_id: PackSegmentId,
    /// Canonical committed frame end bound to the evidence.
    pub tip_frame_end: u64,
    /// Canonical block range and root transition bound to the evidence.
    pub tip_context: PackFrameContext,
    /// Canonical full-frame digest bound to the evidence.
    pub tip_frame_sha256: [u8; 32],
    /// Unique newest-version winner records in the materialized view.
    pub winner_records: u64,
    /// Put winners in the materialized view.
    pub puts: u64,
    /// Tombstone winners in the materialized view.
    pub tombstones: u64,
    /// Put-value bytes addressed by the winner records.
    pub value_bytes: u64,
    /// SHA-256 of every canonical fixed-size winner record in key order.
    pub winner_records_sha256: [u8; 32],
    /// Requested deterministic winner/frame-reference reservoir size.
    pub lookup_sample_requested: u64,
    /// Winner keys exercised through both sorted-batch and point lookup.
    pub lookup_sampled_keys: u64,
    /// Digest of the exact deterministic winner-key sample.
    pub sample_keys_sha256: [u8; 32],
    /// Sampled keys resolving to values.
    pub lookup_present: u64,
    /// Sampled keys resolving to tombstones/absence.
    pub lookup_absent: u64,
    /// Value bytes hashed through sampled lookups.
    pub lookup_value_bytes: u64,
    /// Sampled winner keys additionally cross-checked through point lookup.
    pub point_checks: u64,
    /// Deterministic never-present keys checked through point and batch lookup.
    pub synthetic_miss_checks: u64,
    /// Number of sorted lookup batches issued by this evidence pass.
    ///
    /// This is a physical measurement and is intentionally excluded from
    /// [`Self::state_matches`].
    pub lookup_batches: u64,
    /// Sampled keys independently resolved from committed frame order.
    pub frame_reference_keys: u64,
    /// Full frame scrub performed while resolving independent references.
    pub frame_scrub: PackScrubStats,
    /// Digest of independently resolved frame-reference states.
    pub frame_reference_sha256: [u8; 32],
    /// Domain-separated digest of sampled keys and public lookup results.
    pub lookup_sha256: [u8; 32],
    /// Wall time spent opening and merge-walking every live winner record.
    pub winner_merge_wall_ns: u64,
    /// Wall time spent scrubbing frames and resolving independent references.
    pub frame_reference_wall_ns: u64,
    /// Wall time spent validating sampled point/batch/miss lookups.
    pub lookup_wall_ns: u64,
    /// End-to-end wall time for this complete evidence pass.
    pub total_wall_ns: u64,
}

impl PackMaterializedViewEvidence {
    /// Whether two generations expose exactly the same canonical horizon,
    /// winner records, frame references, and deterministic lookup results.
    /// Physical generation/run/source counts are intentionally excluded.
    pub fn state_matches(&self, other: &Self) -> bool {
        self.tip_epoch == other.tip_epoch
            && self.tip_segment_id == other.tip_segment_id
            && self.tip_frame_end == other.tip_frame_end
            && self.tip_context == other.tip_context
            && self.tip_frame_sha256 == other.tip_frame_sha256
            && self.winner_records == other.winner_records
            && self.puts == other.puts
            && self.tombstones == other.tombstones
            && self.value_bytes == other.value_bytes
            && self.winner_records_sha256 == other.winner_records_sha256
            && self.lookup_sample_requested == other.lookup_sample_requested
            && self.lookup_sampled_keys == other.lookup_sampled_keys
            && self.sample_keys_sha256 == other.sample_keys_sha256
            && self.lookup_present == other.lookup_present
            && self.lookup_absent == other.lookup_absent
            && self.lookup_value_bytes == other.lookup_value_bytes
            && self.point_checks == other.point_checks
            && self.synthetic_miss_checks == other.synthetic_miss_checks
            && self.frame_reference_keys == other.frame_reference_keys
            && self.frame_scrub == other.frame_scrub
            && self.frame_reference_sha256 == other.frame_reference_sha256
            && self.lookup_sha256 == other.lookup_sha256
    }
}

impl PackStore {
    /// Proves that every put-only checkpoint frame row has one identical
    /// materialized index winner and that no additional winner exists.
    ///
    /// The frame side rebuilds canonical positioned index records from
    /// authenticated metadata in segment order. The index side independently
    /// merge-walks every live run with newest-version semantics. Equality of
    /// their complete record digests binds keys, sequences, segment identities,
    /// absolute value offsets, and lengths without retaining either stream.
    pub fn checkpoint_evidence(&self) -> Result<PackCheckpointEvidence> {
        let mut namespace_hasher = Sha256::new();
        namespace_hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        let mut frame_hasher = Sha256::new();
        let mut frame_records = 0u64;
        let mut frame_value_bytes = 0u64;
        let mut previous_key = None;
        let frame_scrub = self.scrub_committed_frames_with(|row| {
            ensure!(
                row.kind == 1,
                "checkpoint index binding requires a put-only frame stream"
            );
            if let Some(previous) = previous_key {
                ensure!(
                    previous < *row.key,
                    "checkpoint index binding requires globally unique ordered frame keys"
                );
            }
            let entry = IndexEntry {
                key: *row.key,
                sequence: row.sequence,
                segment_id: row.segment_id,
                value_offset: row.value_offset,
                value_len: row.value_len,
                tombstone: false,
            };
            namespace_hasher.update((PACK_KEY_BYTES as u32).to_le_bytes());
            namespace_hasher.update(row.key);
            namespace_hasher.update((row.value.len() as u64).to_le_bytes());
            namespace_hasher.update(row.value);
            frame_hasher.update(encode_record(&entry));
            frame_records = frame_records
                .checked_add(1)
                .context("checkpoint frame index-record count overflows u64")?;
            frame_value_bytes = frame_value_bytes
                .checked_add(u64::from(row.value_len))
                .context("checkpoint frame index value bytes overflow u64")?;
            previous_key = Some(*row.key);
            Ok(())
        })?;
        ensure!(
            frame_scrub.rows == frame_records
                && frame_scrub.puts == frame_records
                && frame_scrub.tombstones == 0
                && frame_scrub.value_bytes == frame_value_bytes,
            "checkpoint frame index evidence geometry is inconsistent"
        );
        let frame_records_sha256: [u8; 32] = frame_hasher.finalize().into();

        let winners = self.materialized_view_evidence(0)?;
        ensure!(
            winners.winner_records == frame_records
                && winners.puts == frame_records
                && winners.tombstones == 0
                && winners.value_bytes == frame_value_bytes,
            "checkpoint materialized index geometry differs from committed frame rows"
        );
        ensure!(
            winners.winner_records_sha256 == frame_records_sha256,
            "checkpoint materialized index records differ from committed frame rows"
        );
        Ok(PackCheckpointEvidence {
            namespace: CheckpointNamespaceEvidence {
                scrub: frame_scrub,
                sha256: namespace_hasher.finalize().into(),
            },
            index: PackCheckpointIndexEvidence {
                frame_records,
                winner_records: winners.winner_records,
                value_bytes: frame_value_bytes,
                records_sha256: frame_records_sha256,
                live_runs: winners.live_runs,
                source_records: winners.source_records,
            },
        })
    }

    /// Validates and hashes the complete newest-version winner stream, then
    /// exercises a deterministic bounded sample through independent expected,
    /// point, and sorted-batch paths. Dedicated sequential mappings release
    /// consumed index and frame pages without perturbing point-read mappings.
    pub fn materialized_view_evidence(
        &self,
        lookup_samples: usize,
    ) -> Result<PackMaterializedViewEvidence> {
        let snapshot = self.snapshot()?;
        self.materialized_view_evidence_for_snapshot(snapshot, lookup_samples)
    }

    /// Produces the same complete evidence against a staged compaction view
    /// before its manifest is published. Callers can therefore fail closed
    /// without making an unverified derived generation current.
    pub fn prepared_compaction_evidence(
        &self,
        prepared: &PreparedPackCompaction,
        lookup_samples: usize,
    ) -> Result<PackMaterializedViewEvidence> {
        let snapshot = self.preview_compaction_snapshot(prepared)?;
        self.materialized_view_evidence_for_snapshot(snapshot, lookup_samples)
    }

    fn materialized_view_evidence_for_snapshot(
        &self,
        snapshot: Snapshot,
        lookup_samples: usize,
    ) -> Result<PackMaterializedViewEvidence> {
        ensure!(
            lookup_samples <= MAX_LOOKUP_EVIDENCE_SAMPLES,
            "lookup evidence sample count exceeds the hard limit of {MAX_LOOKUP_EVIDENCE_SAMPLES}"
        );
        let receipt = self
            .last_frame_receipt
            .context("cannot produce materialized evidence for an empty pack")?;
        let total_started = Instant::now();
        let winner_started = Instant::now();
        ensure!(
            !snapshot.runs.is_empty(),
            "cannot produce materialized evidence for an empty run set"
        );
        let mappings = open_evidence_input_maps(&snapshot.runs, &self.runs_dir)?;
        let sources = evidence_sources(&mappings)?;
        let source_records = sources.iter().try_fold(0u64, |total, source| {
            total
                .checked_add(source.record_count)
                .context("evidence source record count overflows")
        })?;
        let mut sampler = EntryReservoir::new(lookup_samples);
        let mut miss_probes = SyntheticMissProbes::new(receipt);
        let mut puts = 0u64;
        let mut tombstones = 0u64;
        let mut value_bytes = 0u64;
        let merged = merge_sorted_runs(&sources, |ordinal, entry, _| {
            validate_entry_payload_range(entry, &snapshot.segments)?;
            sampler.observe(ordinal, *entry);
            miss_probes.observe(&entry.key);
            if entry.tombstone {
                tombstones = tombstones.saturating_add(1);
            } else {
                puts = puts.saturating_add(1);
                value_bytes = value_bytes.saturating_add(u64::from(entry.value_len));
            }
            Ok(())
        })?;
        drop(sources);
        drop(mappings);
        let winner_merge_wall_ns = duration_ns(winner_started.elapsed());

        let entries = sampler.finish();
        let lookup_indices = evenly_spaced_indices(entries.len(), MAX_PUBLIC_LOOKUP_KEYS);
        let lookup_entries: Vec<_> = lookup_indices.iter().map(|&index| entries[index]).collect();
        let sample_keys_sha256 = digest_sample_keys(receipt, &lookup_entries);
        let frame_reference_indices =
            evenly_spaced_indices(entries.len(), MAX_FRAME_REFERENCE_KEYS);
        let frame_reference_started = Instant::now();
        let frame_reference =
            self.resolve_frame_references(receipt, &entries, &frame_reference_indices)?;
        let frame_reference_wall_ns = duration_ns(frame_reference_started.elapsed());
        let synthetic_misses = miss_probes.finish();
        let lookup_started = Instant::now();
        let lookup = digest_lookup_sample(
            &snapshot,
            receipt,
            &lookup_entries,
            &synthetic_misses,
            merged.output_records,
            lookup_samples,
        )?;
        let lookup_wall_ns = duration_ns(lookup_started.elapsed());
        Ok(PackMaterializedViewEvidence {
            generation: snapshot.generation(),
            live_runs: u64::try_from(snapshot.runs.len()).unwrap_or(u64::MAX),
            source_records,
            tip_epoch: receipt.epoch,
            tip_segment_id: receipt.segment_id,
            tip_frame_end: receipt.frame_end,
            tip_context: receipt.context,
            tip_frame_sha256: receipt.frame_sha256,
            winner_records: merged.output_records,
            puts,
            tombstones,
            value_bytes,
            winner_records_sha256: merged.records_sha256,
            lookup_sample_requested: u64::try_from(lookup_samples).unwrap_or(u64::MAX),
            lookup_sampled_keys: u64::try_from(lookup_entries.len()).unwrap_or(u64::MAX),
            sample_keys_sha256,
            lookup_present: lookup.present,
            lookup_absent: lookup.absent,
            lookup_value_bytes: lookup.value_bytes,
            point_checks: lookup.point_checks,
            synthetic_miss_checks: lookup.synthetic_miss_checks,
            lookup_batches: lookup.lookup_batches,
            frame_reference_keys: u64::try_from(frame_reference_indices.len()).unwrap_or(u64::MAX),
            frame_scrub: frame_reference.scrub,
            frame_reference_sha256: frame_reference.sha256,
            lookup_sha256: lookup.sha256,
            winner_merge_wall_ns,
            frame_reference_wall_ns,
            lookup_wall_ns,
            total_wall_ns: duration_ns(total_started.elapsed()),
        })
    }

    fn resolve_frame_references(
        &self,
        receipt: PackFrameReceipt,
        entries: &[IndexEntry],
        indices: &[usize],
    ) -> Result<FrameReferenceEvidence> {
        if indices.is_empty() {
            return Ok(FrameReferenceEvidence::empty(receipt));
        }
        let mut positions = HashMap::with_capacity(indices.len());
        let mut reference_filter = BlockedBloomFilter::with_capacity(
            u64::try_from(indices.len()).context("frame-reference count does not fit u64")?,
            receipt.epoch ^ 0xA076_1D64_78BD_642F,
        )?;
        for (position, &index) in indices.iter().enumerate() {
            reference_filter.insert_hash(key_hash(&entries[index].key));
            ensure!(
                positions.insert(entries[index].key, position).is_none(),
                "frame-reference sample contains a duplicate key"
            );
        }
        let mut states = vec![None; indices.len()];
        let scrub = self.scrub_committed_frames_with(|row| {
            if !blocked_bloom_maybe_contains_hash(
                reference_filter.as_bytes(),
                reference_filter.seed(),
                BLOOM_HASH_PROBES,
                key_hash(row.key),
            ) {
                return Ok(());
            }
            let Some(&position) = positions.get(row.key) else {
                return Ok(());
            };
            states[position] = Some(if row.kind == 0 {
                SampleValueState::Absent
            } else {
                SampleValueState::from_present(row.value)
            });
            Ok(())
        })?;
        let expected_states = resolve_expected_sample_states(&self.segments, entries, indices)?;

        let mut hasher = evidence_hasher(FRAME_REFERENCE_DIGEST_DOMAIN, receipt);
        hasher.update(
            u64::try_from(indices.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        for (position, &index) in indices.iter().enumerate() {
            let entry = entries[index];
            let state = states[position].with_context(|| {
                format!("sampled winner is absent from frames at {:02x?}", entry.key)
            })?;
            let expected = expected_states[position];
            ensure!(
                state == expected,
                "committed-frame and winner-record state disagree at key {:02x?}",
                entry.key
            );
            hash_sample_state(&mut hasher, &entry.key, state);
        }
        Ok(FrameReferenceEvidence {
            scrub,
            sha256: hasher.finalize().into(),
        })
    }
}

struct EvidenceInputMap {
    map: Mmap,
    max_epoch: u64,
    record_count: u64,
    records_offset: usize,
    records_sha256: [u8; 32],
}

fn open_evidence_input_maps(runs: &[LiveRun], runs_dir: &Path) -> Result<Vec<EvidenceInputMap>> {
    let mut mappings = Vec::with_capacity(runs.len());
    for live in runs {
        let path = runs_dir.join(run_file_name(live.level, live.min_epoch, live.max_epoch));
        let file =
            File::open(&path).with_context(|| format!("open evidence input {}", path.display()))?;
        let file_bytes = file
            .metadata()
            .with_context(|| format!("stat evidence input {}", path.display()))?
            .len();
        ensure!(
            file_bytes == live.run.file_bytes,
            "evidence input file length changed"
        );
        mappings.push(EvidenceInputMap {
            map: Mmap::map_sequential(&file, file_bytes, &path)?,
            max_epoch: live.max_epoch,
            record_count: live.run.record_count,
            records_offset: usize::try_from(live.run.records_offset)
                .context("evidence records offset does not fit usize")?,
            records_sha256: live.run.records_sha256,
        });
    }
    Ok(mappings)
}

fn evidence_sources(mappings: &[EvidenceInputMap]) -> Result<Vec<MergeSource<'_>>> {
    mappings
        .iter()
        .map(|input| {
            let record_bytes = usize::try_from(input.record_count)
                .context("evidence record count does not fit usize")?
                .checked_mul(INDEX_RECORD_LEN)
                .context("evidence record byte length overflows")?;
            let end = input
                .records_offset
                .checked_add(record_bytes)
                .context("evidence record range overflows")?;
            let records = input
                .map
                .as_slice()
                .get(input.records_offset..end)
                .context("evidence records lie outside the run")?;
            Ok(MergeSource {
                max_epoch: input.max_epoch,
                record_count: input.record_count,
                records,
                records_sha256: input.records_sha256,
                mapping: Some(&input.map),
                records_offset: input.records_offset,
            })
        })
        .collect()
}

struct EntryReservoir {
    requested: usize,
    rng: XorShift64,
    entries: Vec<IndexEntry>,
}

impl EntryReservoir {
    fn new(requested: usize) -> Self {
        Self {
            requested,
            rng: XorShift64(SAMPLE_SEED),
            entries: Vec::with_capacity(requested),
        }
    }

    fn observe(&mut self, ordinal: u64, entry: IndexEntry) {
        if self.entries.len() < self.requested {
            self.entries.push(entry);
        } else if self.requested != 0 {
            let candidate = self.rng.next() % ordinal.saturating_add(1);
            if candidate < self.requested as u64 {
                self.entries[candidate as usize] = entry;
            }
        }
    }

    fn finish(mut self) -> Vec<IndexEntry> {
        self.entries.sort_unstable_by_key(|entry| entry.key);
        self.entries
    }
}

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
}

struct SyntheticMissProbes {
    probes: Vec<([u8; PACK_KEY_BYTES], bool)>,
    cursor: usize,
}

impl SyntheticMissProbes {
    fn new(receipt: PackFrameReceipt) -> Self {
        let mut probes = Vec::with_capacity(SYNTHETIC_MISS_PROBES);
        for counter in 0..SYNTHETIC_MISS_PROBES as u64 {
            let mut hasher = evidence_hasher(SYNTHETIC_MISS_DIGEST_DOMAIN, receipt);
            hasher.update(counter.to_le_bytes());
            let digest: [u8; 32] = hasher.finalize().into();
            let mut key = [0u8; PACK_KEY_BYTES];
            key[0] = 0xf0;
            key[1..].copy_from_slice(&digest);
            probes.push((key, false));
        }
        probes.sort_unstable_by_key(|probe| probe.0);
        probes.dedup_by_key(|probe| probe.0);
        Self { probes, cursor: 0 }
    }

    fn observe(&mut self, key: &[u8; PACK_KEY_BYTES]) {
        while self
            .probes
            .get(self.cursor)
            .is_some_and(|probe| probe.0 < *key)
        {
            self.cursor += 1;
        }
        if self
            .probes
            .get(self.cursor)
            .is_some_and(|probe| probe.0 == *key)
        {
            self.probes[self.cursor].1 = true;
            self.cursor += 1;
        }
    }

    fn finish(self) -> Vec<[u8; PACK_KEY_BYTES]> {
        self.probes
            .into_iter()
            .filter_map(|(key, seen)| (!seen).then_some(key))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SampleValueState {
    Absent,
    Present { length: u64, sha256: [u8; 32] },
}

impl SampleValueState {
    fn from_present(value: &[u8]) -> Self {
        Self::Present {
            length: value.len() as u64,
            sha256: Sha256::digest(value).into(),
        }
    }
}

struct FrameReferenceEvidence {
    scrub: PackScrubStats,
    sha256: [u8; 32],
}

impl FrameReferenceEvidence {
    fn empty(receipt: PackFrameReceipt) -> Self {
        Self {
            scrub: PackScrubStats::default(),
            sha256: evidence_hasher(FRAME_REFERENCE_DIGEST_DOMAIN, receipt)
                .finalize()
                .into(),
        }
    }
}

struct LookupEvidence {
    present: u64,
    absent: u64,
    value_bytes: u64,
    point_checks: u64,
    synthetic_miss_checks: u64,
    lookup_batches: u64,
    sha256: [u8; 32],
}

fn digest_lookup_sample(
    snapshot: &Snapshot,
    receipt: PackFrameReceipt,
    entries: &[IndexEntry],
    synthetic_misses: &[[u8; PACK_KEY_BYTES]],
    winner_records: u64,
    requested: usize,
) -> Result<LookupEvidence> {
    let point_indices = evenly_spaced_indices(entries.len(), MAX_POINT_CROSS_CHECKS);
    let mut next_point = 0usize;
    let mut present = 0u64;
    let mut absent = 0u64;
    let mut value_bytes = 0u64;
    let mut point_checks = 0u64;
    let mut lookup_batches = 0u64;
    let mut hasher = evidence_hasher(LOOKUP_EVIDENCE_DIGEST_DOMAIN, receipt);
    hasher.update(winner_records.to_le_bytes());
    hasher.update(u64::try_from(requested).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(
        u64::try_from(entries.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );

    let mut start = 0usize;
    while start < entries.len() {
        let (end, _) = next_lookup_batch(entries, start)?;
        let chunk = &entries[start..end];
        let keys: Vec<_> = chunk.iter().map(|entry| entry.key).collect();
        let values = snapshot.get_many_sorted_bounded(
            &keys,
            LOOKUP_BATCH_MAX_VALUE_BYTES,
            LOOKUP_BATCH_MAX_VALUE_BYTES,
        )?;
        ensure!(
            values.len() == chunk.len(),
            "sorted-batch lookup returned {} values for {} keys",
            values.len(),
            chunk.len()
        );
        lookup_batches = lookup_batches.saturating_add(1);
        for (offset, (entry, value)) in chunk.iter().zip(values).enumerate() {
            let global_index = start + offset;
            let expected = expected_value(snapshot, entry)?;
            ensure!(
                value.as_deref() == expected,
                "sorted-batch lookup differs from winner record at key {:02x?}",
                entry.key
            );
            hash_lookup_result(&mut hasher, &entry.key, value.as_deref());
            match value.as_deref() {
                Some(value) => {
                    present = present.saturating_add(1);
                    value_bytes = value_bytes.saturating_add(value.len() as u64);
                }
                None => absent = absent.saturating_add(1),
            }
            if point_indices.get(next_point) == Some(&global_index) {
                ensure!(
                    snapshot
                        .get_bounded(&entry.key, LOOKUP_BATCH_MAX_VALUE_BYTES)?
                        .as_deref()
                        == expected,
                    "point lookup differs from winner record at key {:02x?}",
                    entry.key
                );
                point_checks = point_checks.saturating_add(1);
                next_point += 1;
            }
        }
        snapshot.reclaim_random_lookup_pages()?;
        start = end;
    }
    ensure!(
        present
            .checked_add(absent)
            .context("lookup result count overflows")?
            == u64::try_from(entries.len()).context("lookup sample count does not fit u64")?,
        "sorted-batch lookup result counts do not cover the sample"
    );
    ensure!(
        next_point == point_indices.len(),
        "point cross-check schedule was not fully consumed"
    );

    let mut synthetic_miss_checks = 0u64;
    for chunk in synthetic_misses.chunks(LOOKUP_BATCH_MAX_ENTRIES) {
        let values = snapshot.get_many_sorted_bounded(
            chunk,
            LOOKUP_BATCH_MAX_VALUE_BYTES,
            LOOKUP_BATCH_MAX_VALUE_BYTES,
        )?;
        ensure!(
            values.len() == chunk.len(),
            "synthetic sorted-batch lookup returned {} values for {} keys",
            values.len(),
            chunk.len()
        );
        lookup_batches = lookup_batches.saturating_add(1);
        for (key, value) in chunk.iter().zip(values) {
            ensure!(value.is_none(), "synthetic miss resolved at key {key:02x?}");
            ensure!(
                snapshot
                    .get_bounded(key, LOOKUP_BATCH_MAX_VALUE_BYTES)?
                    .is_none(),
                "synthetic point miss resolved at key {key:02x?}"
            );
            hash_lookup_result(&mut hasher, key, None);
            synthetic_miss_checks = synthetic_miss_checks.saturating_add(1);
        }
        snapshot.reclaim_random_lookup_pages()?;
    }
    ensure!(
        synthetic_miss_checks
            == u64::try_from(synthetic_misses.len())
                .context("synthetic miss count does not fit u64")?,
        "synthetic miss checks did not cover every requested miss"
    );
    Ok(LookupEvidence {
        present,
        absent,
        value_bytes,
        point_checks,
        synthetic_miss_checks,
        lookup_batches,
        sha256: hasher.finalize().into(),
    })
}

/// Selects one bounded sorted-lookup batch and validates every indexed value
/// length before `Snapshot::get_many_sorted` can allocate result buffers.
pub(super) fn next_lookup_batch(entries: &[IndexEntry], start: usize) -> Result<(usize, u64)> {
    ensure!(
        start < entries.len(),
        "lookup batch start is outside the sample"
    );
    let mut end = start;
    let mut value_bytes = 0u64;
    while end < entries.len() && end - start < LOOKUP_BATCH_MAX_ENTRIES {
        let entry = &entries[end];
        let entry_bytes = if entry.tombstone {
            0
        } else {
            u64::from(entry.value_len)
        };
        ensure!(
            entry_bytes <= LOOKUP_BATCH_MAX_VALUE_BYTES,
            "sampled value length {entry_bytes} exceeds the sorted lookup batch limit of {LOOKUP_BATCH_MAX_VALUE_BYTES} bytes"
        );
        let next_value_bytes = value_bytes
            .checked_add(entry_bytes)
            .context("sorted lookup batch value bytes overflow")?;
        if end > start && next_value_bytes > LOOKUP_BATCH_MAX_VALUE_BYTES {
            break;
        }
        value_bytes = next_value_bytes;
        end += 1;
    }
    ensure!(end > start, "sorted lookup batch made no progress");
    Ok((end, value_bytes))
}

fn expected_value<'a>(snapshot: &'a Snapshot, entry: &IndexEntry) -> Result<Option<&'a [u8]>> {
    if entry.tombstone {
        return Ok(None);
    }
    Ok(Some(snapshot.segments.committed_slice(
        PackPosition::new(entry.segment_id, entry.value_offset),
        entry.value_len,
    )?))
}

fn validate_entry_payload_range(entry: &IndexEntry, segments: &SegmentSet) -> Result<()> {
    if entry.tombstone {
        ensure!(
            entry.segment_id == PackSegmentId::INITIAL
                && entry.value_offset == 0
                && entry.value_len == 0,
            "tombstone winner carries a non-canonical value location"
        );
        return Ok(());
    }
    segments.validate_range(
        PackPosition::new(entry.segment_id, entry.value_offset),
        entry.value_len,
    )
}

fn resolve_expected_sample_states(
    segments: &SegmentSet,
    entries: &[IndexEntry],
    indices: &[usize],
) -> Result<Vec<SampleValueState>> {
    let mut states = vec![None; indices.len()];
    let mut scheduled = Vec::with_capacity(indices.len());
    for (position, &index) in indices.iter().enumerate() {
        let entry = entries[index];
        if entry.tombstone {
            states[position] = Some(SampleValueState::Absent);
        } else {
            scheduled.push((entry.segment_id, entry.value_offset, position, entry));
        }
    }
    scheduled
        .sort_unstable_by_key(|&(segment_id, offset, position, _)| (segment_id, offset, position));

    if !scheduled.is_empty() {
        let random_segments = segments.dedicated_random_view()?;
        let mut batch_entries = 0usize;
        let mut batch_value_bytes = 0u64;
        for (_, _, position, entry) in scheduled {
            states[position] = Some(sample_state_from_entry_bounded(&random_segments, &entry)?);
            batch_entries += 1;
            batch_value_bytes = batch_value_bytes.saturating_add(u64::from(entry.value_len));
            if batch_entries >= LOOKUP_BATCH_MAX_ENTRIES
                || batch_value_bytes >= LOOKUP_BATCH_MAX_VALUE_BYTES
            {
                random_segments.reclaim_all_pages()?;
                batch_entries = 0;
                batch_value_bytes = 0;
            }
        }
        random_segments.reclaim_all_pages()?;
    }

    states
        .into_iter()
        .enumerate()
        .map(|(position, state)| {
            state.with_context(|| format!("missing expected state for sample {position}"))
        })
        .collect()
}

fn sample_state_from_entry_bounded(
    segments: &SegmentSet,
    entry: &IndexEntry,
) -> Result<SampleValueState> {
    if entry.tombstone {
        return Ok(SampleValueState::Absent);
    }
    let value = segments.lookup_slice(
        PackPosition::new(entry.segment_id, entry.value_offset),
        entry.value_len,
    )?;
    let mut hasher = Sha256::new();
    for chunk in value.chunks(FRAME_REFERENCE_VALUE_HASH_CHUNK_BYTES) {
        hasher.update(chunk);
    }
    Ok(SampleValueState::Present {
        length: u64::from(entry.value_len),
        sha256: hasher.finalize().into(),
    })
}

fn digest_sample_keys(receipt: PackFrameReceipt, entries: &[IndexEntry]) -> [u8; 32] {
    let mut hasher = evidence_hasher(SAMPLE_KEYSET_DIGEST_DOMAIN, receipt);
    hasher.update(
        u64::try_from(entries.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for entry in entries {
        hasher.update(entry.key);
    }
    hasher.finalize().into()
}

fn evidence_hasher(domain: &[u8], receipt: PackFrameReceipt) -> Sha256 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(receipt.epoch.to_le_bytes());
    hasher.update(receipt.segment_id.get().to_le_bytes());
    hasher.update(receipt.frame_end.to_le_bytes());
    hasher.update(receipt.context.block_start.to_le_bytes());
    hasher.update(receipt.context.block_end.to_le_bytes());
    hasher.update(receipt.context.previous_root);
    hasher.update(receipt.context.resulting_root);
    hasher.update(receipt.frame_sha256);
    hasher
}

fn hash_sample_state(hasher: &mut Sha256, key: &[u8; PACK_KEY_BYTES], state: SampleValueState) {
    hasher.update(key);
    match state {
        SampleValueState::Absent => hasher.update([0]),
        SampleValueState::Present { length, sha256 } => {
            hasher.update([1]);
            hasher.update(length.to_le_bytes());
            hasher.update(sha256);
        }
    }
}

fn hash_lookup_result(hasher: &mut Sha256, key: &[u8; PACK_KEY_BYTES], value: Option<&[u8]>) {
    hasher.update(key);
    match value {
        Some(value) => {
            hasher.update([1]);
            hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
            hasher.update(value);
        }
        None => hasher.update([0]),
    }
}

fn evenly_spaced_indices(length: usize, maximum: usize) -> Vec<usize> {
    let count = length.min(maximum);
    match count {
        0 => Vec::new(),
        1 => vec![0],
        _ => (0..count)
            .map(|index| {
                usize::try_from((index as u128) * ((length - 1) as u128) / ((count - 1) as u128))
                    .expect("evenly spaced index fits usize")
            })
            .collect(),
    }
}

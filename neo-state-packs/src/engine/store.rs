use super::filter::{
    BLOOM_HASH_PROBES, BlockedBloomFilter, blocked_bloom_bytes, blocked_bloom_maybe_contains_hash,
    key_hash, validate_blocked_bloom,
};
use super::manifest::{self, Manifest, ManifestEntry, ManifestExtent, run_file_name};
use super::merge::{
    INDEX_RECORD_LEN, IndexEntry, MergeEvidence, MergeSource, decode_record, encode_record,
    merge_sorted_runs,
};
use super::mmap::Mmap;
use crate::{
    PACK_FRAME_ROW_METADATA_BYTES, PACK_KEY_BYTES, PackOpKind, PackOperation, PackStageTotals,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod api;
pub use api::{
    OpenValidation, PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackCommitHorizon,
    PackFrameContext, PackFrameReceipt, PackPosition, PackSegmentId, PackStoreArtifact,
    PackStoreConfig, PackStoreConfigError, PackStoreConfigField, PackStoreError,
    PackStoreErrorSource, PackStoreLimit, PackStoreOperation, PackStoreOptions, PackStoreResult,
    PreparedAppend, SealedAppend,
};
#[path = "store/format/hashing.rs"]
mod hashing;
use hashing::{digest, index_structure_digest, index_structure_digest_parts};
#[path = "store/lifecycle/io.rs"]
mod io;
use io::{
    clear_interrupted_store_creation, clear_stale_temp_files, preflight_store_creation,
    sync_directory, sync_parent_directory,
};
#[path = "store/lifecycle/lease.rs"]
mod lease;
use lease::acquire_writer_lease;
mod frame_builder;
pub use frame_builder::PackFrameBuilder;
#[path = "store/format/frame_codec.rs"]
mod frame_codec;
use frame_codec::{
    FRAME_METADATA_DIGEST_DOMAIN, FRAME_VALUE_DIGEST_DOMAIN, FramePayloadSection,
    FrameWalkSelection, PendingFrameRow, ValidatedPayloadRow, encode_frame_footer,
    encode_frame_header, encode_frame_payload, encode_pending_rows, frame_digest,
    frame_metadata_digest, frame_value_digest, read_frame_receipt_at,
    scan_frame_metadata_distinct_keys, validate_frame_footer, validate_frame_header,
    validate_payload_rows, validate_payload_rows_detailed_with_progress, verify_frame,
    walk_frames_from_epoch,
};
mod compaction;
use compaction::*;
#[path = "store/format/index_format.rs"]
mod index_format;
use index_format::*;
mod read_view;
use super::metrics::{CompactionDebt, CompactionStats, GcStats, PackMetrics, ReadCounters};
use read_view::ReadView;
pub use read_view::Snapshot;
#[path = "store/validation/scrub.rs"]
mod scrub;
use scrub::*;
#[path = "store/validation/evidence.rs"]
mod evidence;
pub use evidence::PackMaterializedViewEvidence;
#[path = "store/lifecycle/maintenance.rs"]
mod maintenance;
#[path = "store/lifecycle/publication.rs"]
mod publication;
#[path = "store/lifecycle/recovery.rs"]
mod recovery;
#[path = "store/validation/scrub_api.rs"]
mod scrub_api;
#[path = "store/format/segment.rs"]
mod segment;
pub(crate) use segment::initial_segment_exists;
#[path = "store/format/segment_set.rs"]
mod segment_set;
use segment_set::{SegmentMapping, SegmentSet};

const FRAME_MAGIC: &[u8; 8] = b"N3PACK02";
const FRAME_FOOTER_MAGIC: &[u8; 8] = b"N3PKEND2";
const INDEX_MAGIC: &[u8; 8] = b"N3IDXR01";
/// Append-frame format emitted and accepted by this pack engine.
pub const PACK_FRAME_FORMAT_VERSION: u32 = 2;
/// Immutable sorted-index format emitted and accepted by this pack engine.
pub const PACK_INDEX_FORMAT_VERSION: u32 = 5;
const FRAME_HEADER_LEN: usize = 224;
const FRAME_ROW_METADATA_LEN: usize = PACK_FRAME_ROW_METADATA_BYTES;
const FRAME_FOOTER_LEN: usize = 96;
const FRAME_NODE_KEY_PREFIX: u8 = 0xf0;
const INDEX_HEADER_LEN: usize = 192;
const INDEX_STRUCTURE_SHA256_START: usize = 154;
const INDEX_STRUCTURE_SHA256_END: usize = INDEX_STRUCTURE_SHA256_START + 32;
const INDEX_HEADER_TAG_START: usize = 188;
/// Domain separator for a complete checkpoint's ordered key/value digest.
pub const CHECKPOINT_NAMESPACE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs-checkpoint-namespace-v1\0";
const MAX_FRAME_ROWS: u64 = PackStoreConfig::HARD_MAX_FRAME_ROWS;

/// Rejects an invalid operation window before any frame payload allocation.
///
/// The same check is repeated by the byte-level decoder because persisted
/// bytes are untrusted; this boundary check keeps callers from reserving large
/// builder buffers for a context that can never be encoded.
fn validate_frame_context(context: PackFrameContext) -> Result<()> {
    ensure!(
        context.block_start <= context.block_end,
        "frame block range is reversed"
    );
    Ok(())
}

const MAX_FRAME_PAYLOAD_BYTES: u64 = PackStoreConfig::HARD_MAX_FRAME_PAYLOAD_BYTES;
/// Sorted records covered by one sparse fence entry (~3.2 KiB of records).
const FENCE_INTERVAL: usize = 64;
/// Fence entries store the truncated first key of their record block.
const FENCE_KEY_BYTES: usize = 16;
/// Per-run resident metadata charged against the index memory bound.
const RUN_METADATA_BYTES: u64 = 256;
/// Largest level representable by the fixed-width manifest file-name field.
/// Real stores need only logarithmically many levels (well under this cap).
const MAX_COMPACTION_LEVEL: u32 = 99_999;
const COMPACTION_IO_BUFFER_BYTES: usize = 512 * 1024;

static NEXT_STORE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// One immutable sorted run: records stay on disk and are probed through a
/// read-only memory map; only the xor filter and the sparse fences stay
/// resident. `min_prefix`/`max_prefix` are the big-endian leading u64 of the
/// key range so out-of-range keys are rejected with two integer compares.
#[derive(Debug)]
struct IndexRun {
    format_version: u32,
    epoch: u64,
    record_count: u64,
    map: Mmap,
    /// Optional sparse-lookup view carrying random-access readahead advice.
    lookup_map: Option<Mmap>,
    records_offset: u64,
    file_bytes: u64,
    min_key: [u8; PACK_KEY_BYTES],
    max_key: [u8; PACK_KEY_BYTES],
    min_prefix: u64,
    max_prefix: u64,
    fences: Vec<[u8; FENCE_KEY_BYTES]>,
    filter: RunFilter,
    records_sha256: [u8; 32],
    structure_sha256: [u8; 32],
    memory_bytes: u64,
}

#[derive(Debug)]
struct RunFilter {
    seed: u64,
    probes: u32,
    offset: u64,
    bytes: u64,
}

/// One immutable sorted run live in the current manifest generation. Runs
/// are shared with snapshots through `Arc`, so compaction can replace the
/// live set while pinned generations keep reading their own view. Level and
/// epoch range come from the manifest entry; level-0 runs map one append
/// frame exactly (`min_epoch == max_epoch`).
#[derive(Clone, Debug)]
struct LiveRun {
    run: Arc<IndexRun>,
    level: u32,
    min_epoch: u64,
    max_epoch: u64,
}

/// Results from a full sequential scrub of the committed frame prefix.
///
/// A scrub re-hashes every payload and decodes every row header. It does not
/// trust the derived index runs and never changes pack visibility.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackScrubStats {
    /// Committed frames verified in epoch order.
    pub frames: u64,
    /// Put and tombstone rows decoded from committed payloads.
    pub rows: u64,
    /// Put rows decoded from committed payloads.
    pub puts: u64,
    /// Tombstone rows decoded from committed payloads.
    pub tombstones: u64,
    /// Bytes covered by frame payload checksums.
    pub payload_bytes: u64,
    /// Put-value bytes covered by frame payload checksums.
    pub value_bytes: u64,
}

/// Evidence from a full validation of every live derived index run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackIndexScrubStats {
    /// Live runs whose complete record sections were validated.
    pub runs: u64,
    /// Physical v5 blocked-Bloom runs validated.
    pub v5_runs: u64,
    /// Fixed-size records decoded and checksummed.
    pub records: u64,
    /// Record-section bytes covered by SHA-256.
    pub record_bytes: u64,
}

/// Complete binding between checkpoint frame rows and materialized index winners.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackCheckpointIndexEvidence {
    /// Put-only frame rows rebuilt into canonical positioned index records.
    pub frame_records: u64,
    /// Unique materialized winner records merge-walked across all live runs.
    pub winner_records: u64,
    /// Value bytes addressed by both the frame rows and index winners.
    pub value_bytes: u64,
    /// SHA-256 of the exact canonical 64-byte record stream from both sources.
    pub records_sha256: [u8; 32],
    /// Live immutable runs participating in the materialized view.
    pub live_runs: u64,
    /// Physical source records consumed by the newest-winner merge.
    pub source_records: u64,
}

/// Full-payload evidence for an ordered, unique, put-only base checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckpointNamespaceEvidence {
    /// Structural and byte counts produced while hashing every frame row.
    pub scrub: PackScrubStats,
    /// SHA-256 of the canonical ordered checkpoint key/value stream.
    pub sha256: [u8; 32],
}

/// Runs decoded and validated while opening, before tail verification.
#[derive(Default)]
struct LoadedRuns {
    runs: Vec<LiveRun>,
    decoded_index_bytes: u64,
    index_entries: u64,
}

/// A merge result published to disk but not yet adopted by a manifest.
/// Splitting the merge at this point is also how the crash-mid-compaction
/// test stops after the atomic run-file publication.
struct PendingMerge {
    level: u32,
    min_epoch: u64,
    max_epoch: u64,
    run: Arc<IndexRun>,
    input_runs: u64,
    input_records: u64,
    output_records: u64,
    input_memory_bytes: u64,
    inputs: Vec<LiveRun>,
    wall_ns: u64,
}

/// Immutable compaction work that may be built without holding the pack
/// writer lock. Its source snapshot lease prevents reclamation while the
/// derived output is being written and synced.
#[must_use = "a compaction plan must be built or dropped"]
pub struct PackCompactionPlan {
    level: u32,
    inputs: Vec<LiveRun>,
    runs_dir: PathBuf,
    random_point_mmap: bool,
    estimated_workspace_bytes: u64,
    resident_index_bytes: u64,
    max_index_memory_bytes: u64,
    _source_snapshot: Snapshot,
    /// Keeps the deterministic output run alive while build runs outside the
    /// writer lock. Runtime GC consults the same registry before deleting an
    /// unreferenced `.idx` file.
    _output_lease: CompactionOutputLease,
}

/// A fully written and validated derived run awaiting short manifest adoption.
/// The source generation remains leased until this value is adopted or dropped.
#[must_use = "a prepared compaction must be adopted or left for crash-safe cleanup"]
pub struct PreparedPackCompaction {
    pending: PendingMerge,
    _source_snapshot: Snapshot,
    _output_lease: CompactionOutputLease,
}

/// Process-local lease for a compaction output that has been renamed but is
/// not yet referenced by a manifest. The file is durable, but remains
/// invisible until adoption; GC must not delete it in that interval.
struct CompactionOutputLease {
    registry: Arc<Mutex<HashSet<String>>>,
    name: String,
}

impl CompactionOutputLease {
    fn acquire(registry: Arc<Mutex<HashSet<String>>>, name: String) -> Result<Self> {
        let mut outputs = registry
            .lock()
            .map_err(|error| anyhow::anyhow!("compaction output registry is poisoned: {error}"))?;
        ensure!(
            outputs.insert(name.clone()),
            "compaction output {name} is already in flight"
        );
        drop(outputs);
        Ok(Self { registry, name })
    }
}

impl Drop for CompactionOutputLease {
    fn drop(&mut self) {
        if let Ok(mut outputs) = self.registry.lock() {
            outputs.remove(&self.name);
        }
    }
}

impl PackCompactionPlan {
    /// Conservative transient allocation estimate for the current merge
    /// implementation. This is computed only from immutable run metadata.
    pub const fn estimated_workspace_bytes(&self) -> u64 {
        self.estimated_workspace_bytes
    }

    /// Hard workspace bound applied before any input record is read.
    pub const fn max_workspace_bytes(&self) -> u64 {
        self.max_index_memory_bytes
    }

    /// Merges, writes, syncs, and validates the output run. This is the
    /// expensive phase and does not borrow or mutate [`PackStore`].
    pub fn build(self) -> Result<PreparedPackCompaction> {
        let PackCompactionPlan {
            level,
            inputs,
            runs_dir,
            random_point_mmap,
            estimated_workspace_bytes,
            resident_index_bytes,
            max_index_memory_bytes,
            _source_snapshot,
            _output_lease,
        } = self;
        ensure_compaction_workspace(estimated_workspace_bytes, max_index_memory_bytes)?;
        let started = Instant::now();
        let mut pending = build_compacted_run_from_inputs(
            level,
            &inputs,
            &runs_dir,
            random_point_mmap,
            resident_index_bytes,
            max_index_memory_bytes,
        )?;
        pending.inputs = inputs;
        pending.wall_ns = duration_ns(started.elapsed());
        Ok(PreparedPackCompaction {
            pending,
            _source_snapshot,
            _output_lease,
        })
    }
}

/// Durable frame and immutable run waiting for an external commit decision.
/// None of these fields participate in the live read view until activation.
struct PendingAppend {
    token: PreparedAppend,
    run: LiveRun,
    decoded_index_bytes: u64,
}

/// One completely validated next generation. Building this value performs
/// all fallible frame/run validation and allocates every live-view collection
/// before manifest publication.
struct ValidatedAppend {
    receipt: PackFrameReceipt,
    stage_totals: PackStageTotals,
    segments: Arc<SegmentSet>,
    runs: Vec<LiveRun>,
    ranges: Vec<RunRange>,
    decoded_index_bytes: u64,
    next_epoch: u64,
    generation: u64,
    extents: Vec<ManifestExtent>,
    manifest: Manifest,
}

#[derive(Clone, Copy, Debug, Default)]
struct RebuildMetrics {
    frames: u64,
    runs: u64,
    index_entries: u64,
    wall_ns: u64,
}

/// Append-only pack store: identified operation segments, immutable
/// sorted index runs (`runs/`), and immutable manifest generations gating
/// visibility. Single-writer: callers serialize appends (the node shadow
/// writer holds it behind a mutex); readers pin generations through
/// [`Snapshot`] leases.
pub struct PackStore {
    root: PathBuf,
    runs_dir: PathBuf,
    pack: File,
    pack_path: PathBuf,
    /// Segment currently owned by the append writer. It may be one segment
    /// ahead of the visible set while a prepared append awaits activation.
    active_segment_id: PackSegmentId,
    /// Exact authenticated segment prefixes of the visible generation.
    segments: Arc<SegmentSet>,
    /// Live runs of the current manifest generation, sorted by epoch range.
    runs: Vec<LiveRun>,
    /// Small per-level directory used by debt checks on the commit path.
    level_run_counts: BTreeMap<u32, usize>,
    /// Compact per-run key-range directory (16 bytes per run, contiguous):
    /// the newest-first scan touches ~10 cache lines instead of ~130.
    ranges: Vec<RunRange>,
    next_epoch: u64,
    /// Current published manifest generation (0 before the first append).
    generation: u64,
    /// Authenticated frame extents selected by the current manifest.
    extents: Vec<ManifestExtent>,
    decoded_index_bytes: u64,
    config: PackStoreConfig,
    stats: CompactionStats,
    stage_totals: PackStageTotals,
    logical_payload_bytes: u64,
    rebuild: RebuildMetrics,
    read_counters: Arc<ReadCounters>,
    /// Live snapshot leases per manifest generation.
    leases: Arc<Mutex<BTreeMap<u64, usize>>>,
    open_validation: OpenValidation,
    /// Placement/checksum of the newest committed frame.
    last_frame_receipt: Option<PackFrameReceipt>,
    /// At most one durable append may wait for external marker publication.
    pending_append: Option<PendingAppend>,
    /// Process-local identity preventing activation through another handle.
    instance_id: u64,
    next_prepare_serial: u64,
    /// Kernel lease held across recovery, mutation, and derived maintenance.
    _writer_lease: File,
    /// Output names of compactions currently building outside the writer lock.
    inflight_compaction_outputs: Arc<Mutex<HashSet<String>>>,
}

/// Order-equivalent leading-u64 key range of one run.
#[derive(Clone, Copy, Debug)]
struct RunRange {
    min_prefix: u64,
    max_prefix: u64,
}

fn same_live_run(left: &LiveRun, right: &LiveRun) -> bool {
    left.level == right.level
        && left.min_epoch == right.min_epoch
        && left.max_epoch == right.max_epoch
        && Arc::ptr_eq(&left.run, &right.run)
}

fn count_run_levels(runs: &[LiveRun]) -> BTreeMap<u32, usize> {
    let mut counts = BTreeMap::new();
    for live in runs {
        *counts.entry(live.level).or_default() += 1;
    }
    counts
}

fn run_ranges(runs: &[LiveRun]) -> Vec<RunRange> {
    runs.iter()
        .map(|live| RunRange {
            min_prefix: live.run.min_prefix,
            max_prefix: live.run.max_prefix,
        })
        .collect()
}

fn manifest_entry_of(live: &LiveRun) -> ManifestEntry {
    ManifestEntry {
        level: live.level,
        min_epoch: live.min_epoch,
        max_epoch: live.max_epoch,
        format_version: live.run.format_version,
        record_count: live.run.record_count,
        records_offset: live.run.records_offset,
        file_bytes: live.run.file_bytes,
        records_sha256: live.run.records_sha256,
        structure_sha256: live.run.structure_sha256,
        file_name: run_file_name(live.level, live.min_epoch, live.max_epoch),
    }
}

fn next_rebuilt_manifest_generation(newest_generation: Option<u64>) -> Result<u64> {
    newest_generation
        .unwrap_or(0)
        .checked_add(1)
        .context("manifest generation overflows")
}

/// Publishes a rebuilt run set at the generation proved before recovery was
/// allowed to mutate segment or derived artifacts.
fn publish_rebuilt_manifest(
    root: &Path,
    generation: u64,
    extents: &[ManifestExtent],
    loaded: &LoadedRuns,
) -> Result<u64> {
    let entries = loaded.runs.iter().map(manifest_entry_of).collect();
    manifest::publish_manifest(
        root,
        &Manifest {
            generation,
            extents: extents.to_vec(),
            entries,
        },
    )?;
    Ok(generation)
}

fn charge_run_memory(loaded: &mut LoadedRuns, run: &IndexRun, bound: u64) -> Result<()> {
    loaded.decoded_index_bytes = loaded
        .decoded_index_bytes
        .checked_add(run.memory_bytes)
        .context("decoded index bytes overflow")?;
    ensure!(
        loaded.decoded_index_bytes <= bound,
        "decoded index memory {} exceeds configured bound {}",
        loaded.decoded_index_bytes,
        bound
    );
    loaded.index_entries = loaded
        .index_entries
        .checked_add(run.record_count)
        .context("index entry count overflows")?;
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset.checked_add(4).context("u32 offset overflows")?;
    let raw: [u8; 4] = bytes
        .get(offset..end)
        .context("short u32 field")?
        .try_into()
        .expect("four-byte slice");
    Ok(u32::from_le_bytes(raw))
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset.checked_add(2).context("u16 offset overflows")?;
    let raw: [u8; 2] = bytes
        .get(offset..end)
        .context("short u16 field")?
        .try_into()
        .expect("two-byte slice");
    Ok(u16::from_le_bytes(raw))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    let end = offset.checked_add(8).context("u64 offset overflows")?;
    let raw: [u8; 8] = bytes
        .get(offset..end)
        .context("short u64 field")?
        .try_into()
        .expect("eight-byte slice");
    Ok(u64::from_le_bytes(raw))
}

fn duration_ns(duration: std::time::Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn next_store_instance_id() -> u64 {
    NEXT_STORE_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
#[path = "store/tests/mod.rs"]
mod tests;

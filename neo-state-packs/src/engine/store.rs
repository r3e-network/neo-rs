use super::filter::{
    BLOOM_HASH_PROBES, BlockedBloomFilter, FILTER_FINGERPRINT_BITS, XorFilter, blocked_bloom_bytes,
    blocked_bloom_maybe_contains_hash, filter_capacity, key_hash, validate_blocked_bloom,
};
use super::manifest::{self, Manifest, ManifestEntry, run_file_name};
use super::merge::{
    INDEX_RECORD_LEN, IndexEntry, MergeEvidence, MergeSource, decode_record, merge_sorted_runs,
};
use super::mmap::Mmap;
use crate::{PACK_KEY_BYTES, PackOpKind, PackOperation, PackStageTotals};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod frame_builder;
pub use frame_builder::PackFrameBuilder;
mod frame_codec;
use frame_codec::{
    FrameScan, decode_frame_payload, encode_frame_header, encode_frame_payload, read_frame_receipt,
    read_frame_receipt_at, reset_derived_state_to_frame_prefix, scan_frames, validate_frame_header,
    validate_payload_rows_with_progress, verify_tail_frame,
};
mod compaction;
use compaction::*;
mod index_format;
use index_format::*;
mod read_view;
use read_view::ReadView;
pub use read_view::Snapshot;
mod scrub;
use scrub::*;
mod evidence;
pub use evidence::PackMaterializedViewEvidence;
mod maintenance;
mod publication;
mod recovery;
mod scrub_api;

const FRAME_MAGIC: &[u8; 8] = b"N3PACK01";
const INDEX_MAGIC: &[u8; 8] = b"N3IDXR01";
/// Append-frame format emitted and accepted by this pack engine.
pub const PACK_FRAME_FORMAT_VERSION: u32 = 1;
/// Immutable sorted-index format emitted and accepted by this pack engine.
///
/// This remains the canonical marker/checkpoint compatibility family. Physical
/// run headers are versioned independently because derived runs are safely
/// rebuildable from committed frames.
pub const PACK_INDEX_FORMAT_VERSION: u32 = 3;
/// Physical immutable-run format emitted by large-run compaction.
pub const PACK_INDEX_RUN_FORMAT_VERSION: u32 = 4;
const XOR_INDEX_RUN_FORMAT_VERSION: u32 = 3;
const FRAME_HEADER_LEN: usize = 72;
const INDEX_HEADER_LEN: usize = 192;
const INDEX_STRUCTURE_SHA256_START: usize = 154;
const INDEX_STRUCTURE_SHA256_END: usize = INDEX_STRUCTURE_SHA256_START + 32;
const INDEX_HEADER_TAG_START: usize = 188;
const INDEX_STRUCTURE_DIGEST_DOMAIN_V3: &[u8] = b"neo-state-packs/index-structure/v3\0";
const INDEX_STRUCTURE_DIGEST_DOMAIN_V4: &[u8] = b"neo-state-packs/index-structure/v4\0";
/// Domain separator for a complete checkpoint's ordered key/value digest.
pub const CHECKPOINT_NAMESPACE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs-checkpoint-namespace-v1\0";
const FRAME_ROW_HEADER_BYTES: u64 = (PACK_KEY_BYTES + 1 + 4) as u64;
const MAX_FRAME_ROWS: u64 = 4_000_000;
const MAX_FRAME_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const WRITER_LEASE_FILE: &str = "writer.lock";
const WRITER_LEASE_RETRY_ATTEMPTS: usize = 10;
const WRITER_LEASE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);
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

/// Typed operational failures specific to the pack store.
///
/// Other format and I/O failures retain their detailed `anyhow` context.
/// Callers can downcast errors to distinguish active-writer ownership and
/// resource deferral from corruption or ordinary I/O failure.
#[derive(Debug, thiserror::Error)]
pub enum PackStoreError {
    /// Another process or handle owns the recovery and writer lease.
    #[error("node-pack writer is already active for {}", path.display())]
    WriterOwned {
        /// Lease-file path used for the ownership check.
        path: PathBuf,
    },
    /// The operating system could not acquire the kernel lease.
    #[error("failed to acquire node-pack writer lease for {}", path.display())]
    WriterLease {
        /// Lease-file path involved in the failed system call.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// The current in-memory compaction implementation cannot build this
    /// output without exceeding the configured transient workspace bound.
    /// Source runs remain live and no output file has been created.
    #[error(
        "compaction workspace estimate {estimated_bytes} bytes exceeds configured bound {max_bytes} bytes"
    )]
    CompactionWorkspaceExceeded {
        /// Conservative peak allocation estimate for the selected inputs.
        estimated_bytes: u64,
        /// Maximum transient workspace allowed for one compaction build.
        max_bytes: u64,
    },
}

/// Physical read-path options that do not change pack bytes or lookup
/// semantics. Every accelerator is disabled by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackStoreOptions {
    /// Map immutable pack and index files a second time with `MADV_RANDOM`.
    /// All index-located payloads and sparse index-window probes use that view;
    /// compaction, validation, and scrub keep the ordinary mapping.
    pub random_point_mmap: bool,
    /// Workers used to copy values for large sorted batch reads. A value of
    /// one keeps the sequential path. Values above one split only immutable
    /// payload reads; index lookup and result publication remain ordered.
    pub batch_value_workers: usize,
}

impl PackStoreOptions {
    /// Configured worker count capped by the logical CPUs visible to this
    /// process. Failure to query the host fails closed to the sequential path.
    pub fn effective_batch_value_workers(self) -> usize {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        self.batch_value_workers.min(available)
    }

    /// Minimum number of located values required before parallel copying is
    /// worthwhile for this configuration.
    pub fn batch_value_parallel_threshold(self) -> usize {
        self.effective_batch_value_workers()
            .saturating_mul(read_view::PACK_BATCH_VALUES_PER_WORKER)
    }

    fn normalized_for_host(self) -> Self {
        Self {
            random_point_mmap: self.random_point_mmap,
            batch_value_workers: self.effective_batch_value_workers(),
        }
    }
}

impl Default for PackStoreOptions {
    fn default() -> Self {
        Self {
            random_point_mmap: false,
            batch_value_workers: 1,
        }
    }
}

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
    memory_bytes: u64,
}

#[derive(Debug)]
enum RunFilter {
    Xor16(XorFilter),
    BlockedBloom {
        seed: u64,
        probes: u32,
        offset: u64,
        bytes: u64,
    },
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

/// Durable placement and checksum of the most recently appended frame.
///
/// The MDBX high-water marker (see [`crate::shadow`]) records these fields so
/// a later recovery phase can validate the committed pack tip against the
/// canonical marker without re-reading the frame chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFrameReceipt {
    /// Commit epoch of the frame (0-based, contiguous).
    pub epoch: u64,
    /// Absolute byte offset of the frame header inside `frames.pack`.
    pub frame_start: u64,
    /// Absolute byte offset one past the frame payload.
    pub frame_end: u64,
    /// Number of operations encoded in the frame.
    pub rows: u64,
    /// Frame payload length in bytes (without the 72-byte header).
    pub payload_bytes: u64,
    /// SHA-256 checksum of the frame payload, as stored in the frame header.
    pub payload_sha256: [u8; 32],
}

/// Opaque handle for one durable but not-yet-visible append.
///
/// The receipt is suitable for recording in an external canonical commit
/// marker. The handle remains bound to the store instance that prepared it;
/// activation fails closed for stale, duplicated, reordered, or foreign
/// handles.
#[derive(Debug, Clone, Copy)]
pub struct PreparedAppend {
    receipt: PackFrameReceipt,
    stage_totals: PackStageTotals,
    store_instance_id: u64,
    serial: u64,
}

impl PreparedAppend {
    /// Durable frame placement and checksum to record in the external marker.
    pub const fn receipt(self) -> PackFrameReceipt {
        self.receipt
    }

    /// Append and sync work completed by the prepare phase.
    pub const fn stage_totals(self) -> PackStageTotals {
        self.stage_totals
    }

    /// Minimal external commit horizon corresponding to this prepared frame.
    pub const fn commit_horizon(self) -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: self.receipt.epoch,
            payload_sha256: self.receipt.payload_sha256,
        }
    }
}

/// A fully validated and published pack generation awaiting its external
/// canonical commit decision.
///
/// [`PackStore::seal_prepared`] creates and pins the snapshot before returning,
/// so after the caller commits [`Self::commit_horizon`] its only required
/// in-process action is to swap [`Self::into_snapshot`] into the node's read
/// view. The manifest on disk is provisional until that external commit. If
/// the commit fails, the writer must be dropped and reopened through the prior
/// horizon; [`PackStore::open_at_commit_horizon`] then discards this suffix.
#[must_use = "a sealed append must be committed externally or discarded by reopening at the prior horizon"]
pub struct SealedAppend {
    commit_horizon: PackCommitHorizon,
    snapshot: Arc<Snapshot>,
}

impl SealedAppend {
    /// Exact horizon to persist in the external canonical commit marker.
    pub const fn commit_horizon(&self) -> PackCommitHorizon {
        self.commit_horizon
    }

    /// Already-created snapshot for the provisional generation.
    pub const fn snapshot(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    /// Consumes the handoff and returns the snapshot for a non-fallible
    /// post-marker pointer swap.
    pub fn into_snapshot(self) -> Arc<Snapshot> {
        self.snapshot
    }
}

/// Canonical commit horizon supplied by the caller's durable commit marker.
///
/// Pack manifests and index runs are derived visibility aids. A caller that
/// coordinates packs with another authoritative store must reopen through
/// this horizon so a frame published before that store's commit cannot be
/// mistaken for canonical after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackCommitHorizon {
    /// Newest canonically committed frame epoch.
    pub epoch: u64,
    /// SHA-256 checksum of that frame's payload.
    pub payload_sha256: [u8; 32],
}

/// Structural counts observed while opening a pack store.
#[derive(Debug, Clone, Copy)]
pub struct OpenValidation {
    /// Committed frames validated while opening.
    pub frames: u64,
    /// Immutable index runs validated while opening.
    pub runs: u64,
    /// Decoded index records counted while opening.
    pub index_entries: u64,
}

/// Leveled compaction bounds for the derived index runs. Level 0 holds the
/// most recent append runs; when a level exceeds its run bound the oldest
/// runs (up to `fanout`) merge into one run at the next level. Payload
/// frames are never rewritten; compacted records keep pointing at the
/// original frame bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompactionConfig {
    l0_bound: usize,
    l1_bound: usize,
    fanout: usize,
}

impl Default for CompactionConfig {
    /// Every level holds at most 8 runs; one cycle merges up to 16 inputs.
    fn default() -> Self {
        Self {
            l0_bound: 8,
            l1_bound: 8,
            fanout: 16,
        }
    }
}

/// Current derived-index compaction debt.
///
/// The soft bound schedules background maintenance. The hard bound is one
/// additional fanout beyond it; callers must apply backpressure there rather
/// than allowing an unbounded run queue to accumulate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompactionDebt {
    /// Immutable runs referenced by the current manifest.
    pub live_runs: u64,
    /// Runs above their per-level soft bounds.
    pub excess_runs: u64,
    /// Resident filter/fence metadata charged to the configured memory bound.
    pub decoded_index_bytes: u64,
    /// Configured resident metadata hard bound.
    pub max_index_memory_bytes: u64,
    /// A level reached its bounded producer-stall threshold.
    pub backpressure_required: bool,
}

/// Cumulative compaction and reclamation evidence for one pack store.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompactionStats {
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
    /// Wall time spent building and adopting compacted runs.
    pub wall_ns: u64,
    /// High-water mark of concurrently live runs.
    pub peak_live_runs: u64,
    /// Explicit reclamation passes executed.
    pub gc_cycles: u64,
    /// Superseded run files deleted by reclamation.
    pub gc_runs_deleted: u64,
    /// Superseded manifests deleted by reclamation.
    pub gc_manifests_deleted: u64,
    /// Bytes reclaimed by deletion.
    pub gc_bytes_reclaimed: u64,
}

/// One explicit reclamation pass over superseded runs and manifests.
#[derive(Clone, Copy, Debug, Default)]
pub struct GcStats {
    /// Superseded run files deleted in this pass.
    pub runs_deleted: u64,
    /// Superseded manifests deleted in this pass.
    pub manifests_deleted: u64,
    /// Bytes reclaimed by deletion in this pass.
    pub bytes_reclaimed: u64,
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
    /// Physical v3 xor16 runs validated.
    pub v3_runs: u64,
    /// Physical v4 blocked-Bloom runs validated.
    pub v4_runs: u64,
    /// Fixed-size records decoded and checksummed.
    pub records: u64,
    /// Record-section bytes covered by SHA-256.
    pub record_bytes: u64,
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
}

/// A fully written and validated derived run awaiting short manifest adoption.
/// The source generation remains leased until this value is adopted or dropped.
#[must_use = "a prepared compaction must be adopted or left for crash-safe cleanup"]
pub struct PreparedPackCompaction {
    pending: PendingMerge,
    _source_snapshot: Snapshot,
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
        ensure_compaction_workspace(self.estimated_workspace_bytes, self.max_index_memory_bytes)?;
        let started = Instant::now();
        let mut pending = build_compacted_run_from_inputs(
            self.level,
            &self.inputs,
            &self.runs_dir,
            self.random_point_mmap,
            self.resident_index_bytes,
            self.max_index_memory_bytes,
        )?;
        pending.inputs = self.inputs;
        pending.wall_ns = duration_ns(started.elapsed());
        Ok(PreparedPackCompaction {
            pending,
            _source_snapshot: self._source_snapshot,
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
    pack_map: Arc<Mmap>,
    lookup_pack_map: Option<Arc<Mmap>>,
    runs: Vec<LiveRun>,
    ranges: Vec<RunRange>,
    decoded_index_bytes: u64,
    next_epoch: u64,
    generation: u64,
    manifest: Manifest,
}

/// Append-only pack store: one operation stream (`frames.pack`), immutable
/// sorted index runs (`runs/`), and immutable manifest generations gating
/// visibility. Single-writer: callers serialize appends (the node shadow
/// writer holds it behind a mutex); readers pin generations through
/// [`Snapshot`] leases.
pub struct PackStore {
    root: PathBuf,
    runs_dir: PathBuf,
    pack: File,
    pack_path: PathBuf,
    /// Read-only pack mapping, replaced after every append and on open.
    pack_map: Arc<Mmap>,
    /// Separate sparse indexed-read view when random advice is explicitly on.
    lookup_pack_map: Option<Arc<Mmap>>,
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
    decoded_index_bytes: u64,
    max_index_memory_bytes: u64,
    compaction: CompactionConfig,
    options: PackStoreOptions,
    stats: CompactionStats,
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
        record_count: live.run.record_count,
        records_sha256: live.run.records_sha256,
        file_name: run_file_name(live.level, live.min_epoch, live.max_epoch),
    }
}

/// Publishes a rebuilt run set as the next generation after every manifest
/// file currently on disk (including unparseable ones).
fn publish_rebuilt_manifest(
    root: &Path,
    manifests: &[(u64, PathBuf)],
    loaded: &LoadedRuns,
) -> Result<u64> {
    let generation = manifests
        .first()
        .map_or(0, |(generation, _)| *generation)
        .checked_add(1)
        .context("manifest generation overflows")?;
    let entries = loaded.runs.iter().map(manifest_entry_of).collect();
    manifest::publish_manifest(
        root,
        &Manifest {
            generation,
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

fn validate_compaction_config(config: CompactionConfig) -> Result<()> {
    ensure!(
        config.l0_bound >= 1 && config.l1_bound >= 1,
        "compaction level bounds must be non-zero"
    );
    ensure!(config.fanout >= 2, "compaction fanout must exceed one");
    Ok(())
}

fn validate_store_options(options: PackStoreOptions) -> Result<()> {
    ensure!(
        (1..=8).contains(&options.batch_value_workers),
        "batch value workers must be in 1..=8"
    );
    Ok(())
}

/// Deletes leftover temp files from interrupted run or manifest
/// publications. Only `.tmp` artifacts of the prototype's own naming scheme
/// are touched, and only after the live generation is known.
fn clear_stale_temp_files(root: &Path) -> Result<()> {
    for directory in [root.to_path_buf(), root.join("runs")] {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read directory {}", directory.display()));
            }
        };
        for entry in entries {
            let entry = entry.context("read directory entry")?;
            let path = entry.path();
            let is_stale_tmp =
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.ends_with(".tmp")
                            && (name.starts_with("run-") || name.starts_with("manifest-"))
                    });
            if is_stale_tmp {
                fs::remove_file(&path)
                    .with_context(|| format!("delete stale temp file {}", path.display()))?;
            }
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

/// Acquires one kernel-held lease before startup recovery can inspect or
/// mutate pack files. A dedicated inode keeps the lease stable while recovery
/// reopens or truncates `frames.pack`.
fn acquire_writer_lease(root: &Path) -> Result<File> {
    let lease_path = root.join(WRITER_LEASE_FILE);
    let (lease, created) = match OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&lease_path)
    {
        Ok(file) => (file, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_CLOEXEC)
                .open(&lease_path)
                .with_context(|| format!("open writer lease {}", lease_path.display()))?,
            false,
        ),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("create writer lease {}", lease_path.display()));
        }
    };
    for attempt in 0..=WRITER_LEASE_RETRY_ATTEMPTS {
        match lease.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if attempt < WRITER_LEASE_RETRY_ATTEMPTS => {
                // `flock` is inherited between fork and exec. A concurrent
                // subprocess can therefore retain another test/thread's
                // CLOEXEC lease for a few milliseconds after its owner drops.
                std::thread::sleep(WRITER_LEASE_RETRY_DELAY);
            }
            Err(TryLockError::WouldBlock) => {
                return Err(PackStoreError::WriterOwned {
                    path: fs::canonicalize(&lease_path).unwrap_or(lease_path),
                }
                .into());
            }
            Err(TryLockError::Error(source)) => {
                return Err(PackStoreError::WriterLease {
                    path: fs::canonicalize(&lease_path).unwrap_or(lease_path),
                    source,
                }
                .into());
            }
        }
    }
    if created {
        sync_directory(root)?;
    }
    Ok(lease)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn index_structure_digest(
    format_version: u32,
    header: &[u8; INDEX_HEADER_LEN],
    structure: &[u8],
) -> Result<[u8; 32]> {
    let mut hasher = index_structure_hasher(format_version, header)?;
    hasher.update(structure);
    Ok(hasher.finalize().into())
}

fn index_structure_digest_parts(
    format_version: u32,
    header: &[u8; INDEX_HEADER_LEN],
    fences: &[u8],
    filter: &[u8],
) -> Result<[u8; 32]> {
    let mut hasher = index_structure_hasher(format_version, header)?;
    hasher.update(fences);
    hasher.update(filter);
    Ok(hasher.finalize().into())
}

fn index_structure_hasher(format_version: u32, header: &[u8; INDEX_HEADER_LEN]) -> Result<Sha256> {
    let domain = match format_version {
        XOR_INDEX_RUN_FORMAT_VERSION => INDEX_STRUCTURE_DIGEST_DOMAIN_V3,
        PACK_INDEX_RUN_FORMAT_VERSION => INDEX_STRUCTURE_DIGEST_DOMAIN_V4,
        _ => anyhow::bail!("unsupported physical index-run version {format_version}"),
    };
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(&header[..INDEX_STRUCTURE_SHA256_START]);
    hasher.update(&header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]);
    Ok(hasher)
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

include!("store/tests.rs");

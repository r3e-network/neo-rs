use super::filter::{FILTER_FINGERPRINT_BITS, XorFilter, filter_capacity, key_hash};
use super::manifest::{self, Manifest, ManifestEntry, run_file_name};
use super::mmap::Mmap;
use crate::{PACK_KEY_BYTES, PackOpKind, PackOperation, PackStageTotals};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const FRAME_MAGIC: &[u8; 8] = b"N3PACK01";
const INDEX_MAGIC: &[u8; 8] = b"N3IDXR01";
/// Append-frame format emitted and accepted by this pack engine.
pub const PACK_FRAME_FORMAT_VERSION: u32 = 1;
/// Immutable sorted-index format emitted and accepted by this pack engine.
pub const PACK_INDEX_FORMAT_VERSION: u32 = 3;
const FRAME_HEADER_LEN: usize = 72;
const INDEX_HEADER_LEN: usize = 192;
const INDEX_RECORD_LEN: usize = PACK_KEY_BYTES + 4 + 8 + 4 + 1;
const INDEX_STRUCTURE_SHA256_START: usize = 154;
const INDEX_STRUCTURE_SHA256_END: usize = INDEX_STRUCTURE_SHA256_START + 32;
const INDEX_HEADER_TAG_START: usize = 188;
const INDEX_STRUCTURE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/index-structure/v3\0";
/// Domain separator for a complete checkpoint's ordered key/value digest.
pub const CHECKPOINT_NAMESPACE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs-checkpoint-namespace-v1\0";
const FRAME_ROW_HEADER_BYTES: u64 = (PACK_KEY_BYTES + 1 + 4) as u64;
const MAX_FRAME_ROWS: u64 = 4_000_000;
const MAX_FRAME_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const WRITER_LEASE_FILE: &str = "writer.lock";
/// Sorted records covered by one sparse fence entry (~3.2 KiB of records).
const FENCE_INTERVAL: usize = 64;
/// Fence entries store the truncated first key of their record block.
const FENCE_KEY_BYTES: usize = 16;
/// Per-run resident metadata charged against the index memory bound.
const RUN_METADATA_BYTES: u64 = 256;
/// Largest level representable by the fixed-width manifest file-name field.
/// Real stores need only logarithmically many levels (well under this cap).
const MAX_COMPACTION_LEVEL: u32 = 99_999;

static NEXT_STORE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Typed failures specific to pack-store ownership.
///
/// Other format and I/O failures retain their detailed `anyhow` context.
/// Callers can downcast an open error to this type to distinguish an active
/// writer from corruption or ordinary I/O failure.
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
}

/// Physical read-path options that do not change pack bytes or lookup
/// semantics. Every accelerator is disabled by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackStoreOptions {
    /// Map immutable pack and index files a second time with `MADV_RANDOM`.
    /// All index-located payloads and sparse index-window probes use that view;
    /// compaction, validation, and scrub keep the ordinary mapping.
    pub random_point_mmap: bool,
}

#[derive(Clone, Copy, Debug)]
struct IndexEntry {
    key: [u8; PACK_KEY_BYTES],
    sequence: u32,
    value_offset: u64,
    value_len: u32,
    tombstone: bool,
}

/// One immutable sorted run: records stay on disk and are probed through a
/// read-only memory map; only the xor filter and the sparse fences stay
/// resident. `min_prefix`/`max_prefix` are the big-endian leading u64 of the
/// key range so out-of-range keys are rejected with two integer compares.
#[derive(Debug)]
struct IndexRun {
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
    filter: XorFilter,
    records_sha256: [u8; 32],
    memory_bytes: u64,
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
    run: IndexRun,
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
    /// Merges, writes, syncs, and validates the output run. This is the
    /// expensive phase and does not borrow or mutate [`PackStore`].
    pub fn build(self) -> Result<PreparedPackCompaction> {
        let started = Instant::now();
        let mut pending = build_compacted_run_from_inputs(
            self.level,
            &self.inputs,
            &self.runs_dir,
            self.random_point_mmap,
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

impl PackStore {
    /// Creates an empty pack store in `root` (which must be missing or empty)
    /// with the default leveled-compaction bounds.
    pub fn create(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        Self::create_with_compaction(root, max_index_memory_bytes, CompactionConfig::default())
    }

    /// Creates an empty store with explicit physical read-path options.
    pub fn create_with_options(
        root: &Path,
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<Self> {
        Self::create_with_compaction_and_options(
            root,
            max_index_memory_bytes,
            CompactionConfig::default(),
            options,
        )
    }

    fn create_with_compaction(
        root: &Path,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
    ) -> Result<Self> {
        Self::create_with_compaction_and_options(
            root,
            max_index_memory_bytes,
            compaction,
            PackStoreOptions::default(),
        )
    }

    fn create_with_compaction_and_options(
        root: &Path,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
        options: PackStoreOptions,
    ) -> Result<Self> {
        ensure!(
            max_index_memory_bytes > 0,
            "index memory bound must be non-zero"
        );
        validate_compaction_config(compaction)?;
        if root.exists() {
            ensure!(
                fs::read_dir(root)
                    .with_context(|| format!("read pack store directory {}", root.display()))?
                    .next()
                    .is_none(),
                "pack store directory must be empty: {}",
                root.display()
            );
        } else {
            fs::create_dir_all(root)
                .with_context(|| format!("create pack store directory {}", root.display()))?;
        }
        let writer_lease = acquire_writer_lease(root)?;
        let runs_dir = root.join("runs");
        fs::create_dir(&runs_dir)
            .with_context(|| format!("create index-run directory {}", runs_dir.display()))?;
        let pack_path = root.join("frames.pack");
        let pack = OpenOptions::new()
            .create_new(true)
            .read(true)
            .append(true)
            .open(&pack_path)
            .with_context(|| format!("create append pack {}", pack_path.display()))?;
        sync_directory(root)?;
        let pack_map = Mmap::map(&pack, 0, &pack_path)?;
        let lookup_pack_map = map_random_if_enabled(&pack, 0, &pack_path, options)?;
        Ok(Self {
            root: root.to_path_buf(),
            runs_dir,
            pack,
            pack_path,
            pack_map: Arc::new(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::new),
            runs: Vec::new(),
            level_run_counts: BTreeMap::new(),
            ranges: Vec::new(),
            next_epoch: 0,
            generation: 0,
            decoded_index_bytes: 0,
            max_index_memory_bytes,
            compaction,
            options,
            stats: CompactionStats::default(),
            leases: Arc::new(Mutex::new(BTreeMap::new())),
            open_validation: OpenValidation {
                frames: 0,
                runs: 0,
                index_entries: 0,
            },
            last_frame_receipt: None,
            pending_append: None,
            instance_id: next_store_instance_id(),
            next_prepare_serial: 0,
            _writer_lease: writer_lease,
        })
    }

    /// Opens a store through the newest manifest generation with structural
    /// frame validation, committed-tail verification, and per-run structure
    /// checks. Index records are not decoded into memory and payloads are
    /// not re-hashed; older committed frames were verified when written and
    /// are re-checked by scrubbing. Missing or corrupt derived indexes are
    /// rebuilt from committed frames (a slow but correct recovery path);
    /// a manifest ahead of the validated frame chain is fatal.
    pub fn open(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        Self::open_with_compaction(root, max_index_memory_bytes, CompactionConfig::default())
    }

    /// Opens the newest visible generation with explicit physical read-path
    /// options. The options are not part of the durable format identity.
    pub fn open_with_options(
        root: &Path,
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<Self> {
        Self::open_with_compaction_and_options(
            root,
            max_index_memory_bytes,
            CompactionConfig::default(),
            options,
        )
    }

    /// Opens a pack at the exact horizon selected by an external durable
    /// commit marker.
    ///
    /// A missing horizon means no frame is canonical. Complete frames or
    /// manifests beyond the horizon are orphan suffixes: the pack is
    /// truncated and all derived indexes are rebuilt from the retained
    /// prefix. A marker that names absent or checksum-mismatched bytes fails
    /// closed.
    pub fn open_at_commit_horizon(
        root: &Path,
        max_index_memory_bytes: u64,
        horizon: Option<PackCommitHorizon>,
    ) -> Result<Self> {
        Self::open_at_commit_horizon_with_options(
            root,
            max_index_memory_bytes,
            horizon,
            PackStoreOptions::default(),
        )
    }

    /// Opens at an externally committed horizon with explicit physical
    /// read-path options. Recovery and canonical visibility are unchanged.
    pub fn open_at_commit_horizon_with_options(
        root: &Path,
        max_index_memory_bytes: u64,
        horizon: Option<PackCommitHorizon>,
        options: PackStoreOptions,
    ) -> Result<Self> {
        ensure!(
            max_index_memory_bytes > 0,
            "index memory bound must be non-zero"
        );
        let writer_lease = acquire_writer_lease(root)?;
        let pack_path = root.join("frames.pack");
        let pack = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&pack_path)
            .with_context(|| format!("open append pack {}", pack_path.display()))?;
        let scan = scan_frames(&pack)?;
        let expected_frames = match horizon {
            Some(horizon) => horizon
                .epoch
                .checked_add(1)
                .context("committed pack epoch overflows")?,
            None => 0,
        };
        ensure!(
            expected_frames <= scan.frame_ends.len() as u64,
            "pack commit marker requires {expected_frames} frames but only {} complete frames exist",
            scan.frame_ends.len()
        );
        if let Some(horizon) = horizon {
            let receipt = read_frame_receipt(&pack, &scan, horizon.epoch)?;
            ensure!(
                receipt.payload_sha256 == horizon.payload_sha256,
                "pack commit marker checksum does not match frame {}",
                horizon.epoch
            );
        }

        let manifests = manifest::list_manifest_files(root)?;
        let manifest_frames = manifests
            .first()
            .and_then(|(_, path)| manifest::read_manifest(path).ok())
            .and_then(|manifest| manifest.max_epoch().checked_add(1));
        let fast_open = if expected_frames == 0 {
            scan.frame_ends.is_empty()
                && manifests.is_empty()
                && fs::read_dir(root.join("runs"))
                    .context("read index-run directory for empty horizon")?
                    .next()
                    .is_none()
        } else {
            manifest_frames == Some(expected_frames)
        };
        drop(pack);

        if !fast_open {
            reset_derived_state_to_frame_prefix(root, &scan, expected_frames)?;
            if expected_frames > 0 {
                let recovered_pack = OpenOptions::new()
                    .read(true)
                    .append(true)
                    .open(&pack_path)
                    .with_context(|| {
                        format!(
                            "open append pack {} for marker rebuild",
                            pack_path.display()
                        )
                    })?;
                let recovered_scan = scan_frames(&recovered_pack)?;
                ensure!(
                    recovered_scan.frame_ends.len() as u64 == expected_frames,
                    "marker recovery retained {} frames, expected {expected_frames}",
                    recovered_scan.frame_ends.len()
                );
                let loaded = Self::rebuild_runs_from_frames(
                    &recovered_pack,
                    &recovered_scan.frame_ends,
                    &root.join("runs"),
                    max_index_memory_bytes,
                    options,
                )?;
                let manifests = manifest::list_manifest_files(root)?;
                ensure!(
                    manifests.is_empty(),
                    "marker recovery did not remove old manifests"
                );
                publish_rebuilt_manifest(root, &manifests, &loaded)?;
            }
        }

        let store = Self::open_with_compaction_and_lease(
            root,
            max_index_memory_bytes,
            CompactionConfig::default(),
            options,
            writer_lease,
        )?;
        ensure!(
            store.open_validation.frames == expected_frames,
            "recovered pack exposes {} frames, expected {expected_frames}",
            store.open_validation.frames
        );
        match (horizon, store.last_frame_receipt) {
            (Some(horizon), Some(receipt)) => ensure!(
                receipt.epoch == horizon.epoch && receipt.payload_sha256 == horizon.payload_sha256,
                "recovered pack tail does not match the canonical commit marker"
            ),
            (Some(_), None) => anyhow::bail!("recovered pack has no committed tail frame"),
            (None, Some(_)) => anyhow::bail!("uncommitted pack frames remain visible"),
            (None, None) => {}
        }
        Ok(store)
    }

    fn open_with_compaction(
        root: &Path,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
    ) -> Result<Self> {
        Self::open_with_compaction_and_options(
            root,
            max_index_memory_bytes,
            compaction,
            PackStoreOptions::default(),
        )
    }

    fn open_with_compaction_and_options(
        root: &Path,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
        options: PackStoreOptions,
    ) -> Result<Self> {
        let writer_lease = acquire_writer_lease(root)?;
        Self::open_with_compaction_and_lease(
            root,
            max_index_memory_bytes,
            compaction,
            options,
            writer_lease,
        )
    }

    fn open_with_compaction_and_lease(
        root: &Path,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
        options: PackStoreOptions,
        writer_lease: File,
    ) -> Result<Self> {
        ensure!(
            max_index_memory_bytes > 0,
            "index memory bound must be non-zero"
        );
        validate_compaction_config(compaction)?;
        let runs_dir = root.join("runs");
        let pack_path = root.join("frames.pack");
        let pack = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&pack_path)
            .with_context(|| format!("open append pack {}", pack_path.display()))?;
        let scan = scan_frames(&pack)?;
        let frame_count =
            u64::try_from(scan.frame_ends.len()).context("frame count does not fit u64")?;
        let manifests = manifest::list_manifest_files(root)?;
        let mut generation = 0u64;
        let loaded = match manifests.first() {
            Some((_, path)) => {
                let current = manifest::read_manifest(path).with_context(|| {
                    format!(
                        "read newest manifest {}; visibility authority is unavailable",
                        path.display()
                    )
                })?;
                ensure!(
                    current.max_epoch() < frame_count,
                    "manifest generation {} commits {} frames but only {} validated in the pack",
                    current.generation,
                    current.max_epoch() + 1,
                    frame_count
                );
                generation = current.generation;
                match Self::load_manifest_runs(&runs_dir, &current, max_index_memory_bytes, options)
                {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        // Indexes are derived, but only the manifest's exact
                        // visible prefix may be rebuilt without an external
                        // canonical marker. Raw frames beyond it stay orphaned.
                        let prefix = &scan.frame_ends[..=usize::try_from(current.max_epoch())
                            .context("manifest epoch does not fit usize")?];
                        let loaded = Self::rebuild_runs_from_frames(
                            &pack,
                            prefix,
                            &runs_dir,
                            max_index_memory_bytes,
                            options,
                        )
                        .with_context(|| format!("rebuild manifest index runs: {error:#}"))?;
                        generation = publish_rebuilt_manifest(root, &manifests, &loaded)?;
                        loaded
                    }
                }
            }
            // Without a manifest or an explicit external horizon there is no
            // durable commit decision. Complete frames and runs are prepared
            // orphan data and must remain invisible.
            None => LoadedRuns::default(),
        };
        Self::finish_open(
            root,
            pack,
            pack_path,
            scan,
            generation,
            loaded,
            max_index_memory_bytes,
            compaction,
            options,
            writer_lease,
        )
    }

    /// Loads every run listed in one manifest generation and cross-checks
    /// record counts and records checksums against the manifest entries.
    fn load_manifest_runs(
        runs_dir: &Path,
        current: &Manifest,
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<LoadedRuns> {
        let mut loaded = LoadedRuns::default();
        for entry in &current.entries {
            let run = read_index_run_with_options(&runs_dir.join(&entry.file_name), options)?;
            ensure!(
                run.epoch == entry.max_epoch
                    && run.record_count == entry.record_count
                    && run.records_sha256 == entry.records_sha256,
                "manifest entry does not match run {}",
                entry.file_name
            );
            charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
            loaded.runs.push(LiveRun {
                run: Arc::new(run),
                level: entry.level,
                min_epoch: entry.min_epoch,
                max_epoch: entry.max_epoch,
            });
        }
        Ok(loaded)
    }

    /// Rebuilds one level-0 run per committed frame directly from the pack.
    /// Every frame payload is re-hashed and decoded; this is the slow
    /// recovery path, never the steady-state open.
    fn rebuild_runs_from_frames(
        pack: &File,
        frame_ends: &[u64],
        runs_dir: &Path,
        max_index_memory_bytes: u64,
        options: PackStoreOptions,
    ) -> Result<LoadedRuns> {
        let mut loaded = LoadedRuns::default();
        let mut frame_start = 0u64;
        for (epoch, frame_end) in frame_ends.iter().enumerate() {
            let epoch = u64::try_from(epoch).context("rebuilt epoch does not fit u64")?;
            let mut header = [0u8; FRAME_HEADER_LEN];
            pack.read_exact_at(&mut header, frame_start)
                .context("re-read frame header for index rebuild")?;
            let payload_len = validate_frame_header(&header, epoch)?;
            let payload_end = frame_start
                .checked_add(FRAME_HEADER_LEN as u64)
                .and_then(|end| end.checked_add(payload_len))
                .context("rebuilt frame end overflows")?;
            ensure!(
                payload_end == *frame_end,
                "rebuilt frame length mismatch at epoch {epoch}"
            );
            let mut payload = vec![0u8; usize::try_from(payload_len).context("payload too large")?];
            pack.read_exact_at(&mut payload, frame_start + FRAME_HEADER_LEN as u64)
                .context("read frame payload for index rebuild")?;
            ensure!(
                digest(&payload).as_slice() == &header[40..72],
                "frame payload checksum mismatch during index rebuild"
            );
            let mut entries = decode_frame_payload(frame_start, &payload)?;
            entries.sort_unstable_by(|left, right| {
                left.key
                    .cmp(&right.key)
                    .then_with(|| left.sequence.cmp(&right.sequence))
            });
            let file_name = run_file_name(0, epoch, epoch);
            let run = publish_fresh_run(&entries, epoch, runs_dir, &file_name, options)
                .with_context(|| format!("rebuild index run for frame {epoch}"))?;
            charge_run_memory(&mut loaded, &run, max_index_memory_bytes)?;
            loaded.runs.push(LiveRun {
                run: Arc::new(run),
                level: 0,
                min_epoch: epoch,
                max_epoch: epoch,
            });
            frame_start = *frame_end;
        }
        Ok(loaded)
    }

    /// Shared open tail: truncate everything past the committed frame prefix,
    /// map the pack, and fully verify the committed tail frame and tail run.
    fn finish_open(
        root: &Path,
        pack: File,
        pack_path: PathBuf,
        scan: FrameScan,
        generation: u64,
        loaded: LoadedRuns,
        max_index_memory_bytes: u64,
        compaction: CompactionConfig,
        options: PackStoreOptions,
        writer_lease: File,
    ) -> Result<Self> {
        let last_frame_receipt = loaded
            .runs
            .last()
            .map(|tail| read_frame_receipt(&pack, &scan, tail.max_epoch))
            .transpose()?;
        let committed_end = match loaded.runs.last() {
            Some(tail) => {
                scan.frame_ends[usize::try_from(tail.max_epoch)
                    .context("committed epoch does not fit usize")?]
            }
            None => 0,
        };
        // A frame becomes visible only with its published manifest. Truncate
        // torn tail bytes and any frames whose publication was interrupted.
        if pack.metadata().context("stat append pack")?.len() != committed_end {
            pack.set_len(committed_end)
                .context("truncate append pack to committed frames")?;
            pack.sync_data().context("sync truncated append pack")?;
        }
        let pack_map = Mmap::map(&pack, committed_end, &pack_path)?;
        let lookup_pack_map = map_random_if_enabled(&pack, committed_end, &pack_path, options)?;
        // Interrupted publications leave stale temp files behind; clearing
        // them here keeps the create-new publication steps from tripping
        // over a crashed predecessor's leftovers.
        clear_stale_temp_files(root)?;
        if let Some(tail) = loaded.runs.last() {
            let tail_start = if tail.max_epoch == 0 {
                0
            } else {
                scan.frame_ends[usize::try_from(tail.max_epoch - 1)
                    .context("previous epoch does not fit usize")?]
            };
            verify_tail_frame(&pack_map, tail_start, committed_end, tail.max_epoch)?;
            verify_tail_run(&tail.run)?;
        }
        let ranges = loaded
            .runs
            .iter()
            .map(|live| RunRange {
                min_prefix: live.run.min_prefix,
                max_prefix: live.run.max_prefix,
            })
            .collect();
        let frames = loaded.runs.last().map_or(0, |tail| tail.max_epoch + 1);
        let run_count = u64::try_from(loaded.runs.len()).context("run count does not fit u64")?;
        let stats = CompactionStats {
            peak_live_runs: run_count,
            ..CompactionStats::default()
        };
        let level_run_counts = count_run_levels(&loaded.runs);
        Ok(Self {
            root: root.to_path_buf(),
            runs_dir: root.join("runs"),
            pack,
            pack_path,
            pack_map: Arc::new(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::new),
            runs: loaded.runs,
            level_run_counts,
            ranges,
            next_epoch: frames,
            generation,
            decoded_index_bytes: loaded.decoded_index_bytes,
            max_index_memory_bytes,
            compaction,
            options,
            stats,
            leases: Arc::new(Mutex::new(BTreeMap::new())),
            open_validation: OpenValidation {
                frames,
                runs: run_count,
                index_entries: loaded.index_entries,
            },
            last_frame_receipt,
            pending_append: None,
            instance_id: next_store_instance_id(),
            next_prepare_serial: 0,
            _writer_lease: writer_lease,
        })
    }

    /// Compatibility append API: durably prepares one frame and then
    /// immediately activates it through its matching commit horizon.
    pub fn append(&mut self, operations: &[PackOperation]) -> Result<PackStageTotals> {
        let prepared = self.prepare_append(operations)?;
        let totals = prepared.stage_totals();
        self.activate_prepared(prepared, prepared.commit_horizon())?;
        self.maintain()?;
        Ok(totals)
    }

    /// Writes and durably syncs one frame and its immutable level-0 run
    /// without changing the manifest, live read view, epoch, or visible tail.
    ///
    /// The returned token supplies the receipt for an external canonical
    /// marker. Exactly one prepared append may exist at a time; callers must
    /// activate it, or drop and reopen the store to discard the orphan suffix.
    pub fn prepare_append(&mut self, operations: &[PackOperation]) -> Result<PreparedAppend> {
        ensure!(!operations.is_empty(), "append frame must not be empty");
        ensure!(
            self.pending_append.is_none(),
            "a prepared append is already awaiting activation"
        );
        let physical_len = self.pack.metadata().context("stat append pack")?.len();
        let visible_len = u64::try_from(self.pack_map.as_slice().len())
            .context("visible pack length does not fit u64")?;
        ensure!(
            physical_len == visible_len,
            "append pack contains an unresolved orphan suffix; reopen before preparing another frame"
        );
        let epoch = self.next_epoch;
        let next_prepare_serial = self
            .next_prepare_serial
            .checked_add(1)
            .context("prepared append serial overflows")?;
        let frame_start = physical_len;
        let (payload, mut entries) = encode_frame_payload(frame_start, operations)?;
        entries.sort_unstable_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.sequence.cmp(&right.sequence))
        });
        let keys = distinct_keys(&entries);
        let structured = run_structured_bytes(entries.len(), keys.len())?;
        let prospective = self
            .decoded_index_bytes
            .checked_add(structured)
            .context("decoded index bytes overflow")?;
        ensure!(
            prospective <= self.max_index_memory_bytes,
            "decoded index memory {prospective} exceeds configured bound {}",
            self.max_index_memory_bytes
        );
        let payload_checksum = digest(&payload);
        let header = encode_frame_header(epoch, operations.len(), payload.len(), payload_checksum)?;

        let write_started = Instant::now();
        self.pack.write_all(&header).context("write frame header")?;
        self.pack
            .write_all(&payload)
            .context("write frame payload")?;
        let append_write_ns = duration_ns(write_started.elapsed());
        let sync_started = Instant::now();
        self.pack.sync_data().context("sync append pack frame")?;
        let pack_sync_ns = duration_ns(sync_started.elapsed());
        let pack_len = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload.len() as u64))
            .context("appended pack length overflows")?;
        let receipt = PackFrameReceipt {
            epoch,
            frame_start,
            frame_end: pack_len,
            rows: u64::try_from(operations.len()).context("frame row count does not fit u64")?,
            payload_bytes: u64::try_from(payload.len())
                .context("frame payload length does not fit u64")?,
            payload_sha256: payload_checksum,
        };

        let min_key = entries.first().expect("non-empty frame").key;
        let max_key = entries.last().expect("non-empty frame").key;
        let fences = build_fences(&entries);
        let filter =
            XorFilter::build(&keys, filter_seed(epoch)).context("build run membership filter")?;
        let (index_bytes, records_sha256) =
            encode_index_run(epoch, &entries, &fences, &filter, &min_key, &max_key)?;
        let final_path = self.runs_dir.join(run_file_name(0, epoch, epoch));
        let temp_path = self.runs_dir.join(format!("run-{epoch:020}.tmp"));
        let index_write_started = Instant::now();
        let mut index_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| format!("create index run {}", temp_path.display()))?;
        index_file
            .write_all(&index_bytes)
            .with_context(|| format!("write index run {}", temp_path.display()))?;
        let index_write_ns = duration_ns(index_write_started.elapsed());
        let index_sync_started = Instant::now();
        index_file
            .sync_data()
            .with_context(|| format!("sync index run {}", temp_path.display()))?;
        let index_sync_ns = duration_ns(index_sync_started.elapsed());
        drop(index_file);
        fs::rename(&temp_path, &final_path).with_context(|| {
            format!(
                "publish index run {} as {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
        let directory_sync_started = Instant::now();
        sync_directory(&self.runs_dir)?;
        let directory_sync_ns = duration_ns(directory_sync_started.elapsed());

        let file = File::open(&final_path)
            .with_context(|| format!("open published index run {}", final_path.display()))?;
        let file_bytes = u64::try_from(index_bytes.len()).context("index bytes do not fit u64")?;
        let map = Mmap::map(&file, file_bytes, &final_path)?;
        drop(file);
        let records_offset = (INDEX_HEADER_LEN
            + fences.len() * FENCE_KEY_BYTES
            + filter.fingerprint_count() * 2) as u64;
        let run = LiveRun {
            run: Arc::new(IndexRun {
                epoch,
                record_count: u64::try_from(entries.len())
                    .context("index count does not fit u64")?,
                map,
                // Pending runs are not read-visible. `validate_prepared`
                // creates the advised map before external marker commit.
                lookup_map: None,
                records_offset,
                file_bytes,
                min_key,
                max_key,
                min_prefix: key_prefix(&min_key),
                max_prefix: key_prefix(&max_key),
                fences,
                filter,
                records_sha256,
                memory_bytes: structured,
            }),
            level: 0,
            min_epoch: epoch,
            max_epoch: epoch,
        };
        let stage_totals = PackStageTotals {
            append_write_ns,
            pack_sync_ns,
            index_write_ns,
            index_sync_ns,
            directory_sync_ns,
            frames: 1,
            index_entries: u64::try_from(operations.len())
                .context("operation count does not fit u64")?,
        };
        let token = PreparedAppend {
            receipt,
            stage_totals,
            store_instance_id: self.instance_id,
            serial: self.next_prepare_serial,
        };
        self.pending_append = Some(PendingAppend {
            token,
            run,
            decoded_index_bytes: prospective,
        });
        self.next_prepare_serial = next_prepare_serial;
        Ok(token)
    }

    /// Completes every fallible pack operation before an external canonical
    /// marker commit.
    ///
    /// The frame and run are revalidated, the next manifest is durably
    /// published, and its read snapshot is pinned before this method returns.
    /// That manifest and this store handle's current generation are
    /// provisional until the caller commits [`SealedAppend::commit_horizon`].
    /// After a successful external commit, consuming the sealed handoff for a
    /// pointer swap performs no I/O, validation, locking, or allocation.
    ///
    /// If the external commit fails, callers must not append again through
    /// this handle. Drop it and reopen through the preceding canonical horizon;
    /// recovery will discard the sealed suffix and its provisional manifest.
    pub fn seal_prepared(&mut self, prepared: PreparedAppend) -> Result<SealedAppend> {
        let commit_horizon = prepared.commit_horizon();
        let validated = self.validate_prepared(prepared, commit_horizon)?;
        let snapshot = Arc::new(self.pin_snapshot_parts(
            validated.generation,
            &validated.runs,
            &validated.ranges,
            &validated.pack_map,
            validated.lookup_pack_map.as_ref(),
        )?);
        manifest::publish_manifest(&self.root, &validated.manifest)?;
        self.install_validated_append(validated);
        Ok(SealedAppend {
            commit_horizon,
            snapshot,
        })
    }

    /// Activates a prepared append after the caller has durably committed the
    /// matching external marker.
    ///
    /// This post-marker compatibility path is retained for shadow mode. New
    /// authoritative coordination should use [`Self::seal_prepared`] so every
    /// fallible pack operation completes before the marker commit.
    pub fn activate_prepared(
        &mut self,
        prepared: PreparedAppend,
        committed: PackCommitHorizon,
    ) -> Result<()> {
        let validated = self.validate_prepared(prepared, committed)?;
        manifest::publish_manifest(&self.root, &validated.manifest)?;
        self.install_validated_append(validated);
        Ok(())
    }

    /// Revalidates the pending durable frame and run and constructs the next
    /// immutable generation without publishing it.
    fn validate_prepared(
        &self,
        prepared: PreparedAppend,
        committed: PackCommitHorizon,
    ) -> Result<ValidatedAppend> {
        ensure!(
            prepared.store_instance_id == self.instance_id,
            "prepared append belongs to another pack-store handle"
        );
        let pending = self
            .pending_append
            .as_ref()
            .context("no prepared append is awaiting activation")?;
        ensure!(
            pending.token.serial == prepared.serial
                && pending.token.store_instance_id == prepared.store_instance_id
                && pending.token.receipt == prepared.receipt,
            "prepared append token does not match the pending frame"
        );
        ensure!(
            committed.epoch == prepared.receipt.epoch,
            "external commit marker epoch does not match the prepared frame"
        );
        ensure!(
            committed.payload_sha256 == prepared.receipt.payload_sha256,
            "external commit marker checksum does not match the prepared frame"
        );
        ensure!(
            prepared.receipt.epoch == self.next_epoch,
            "prepared frame activation is out of order"
        );
        let next_epoch = self
            .next_epoch
            .checked_add(1)
            .context("append epoch overflows")?;
        let generation = self
            .generation
            .checked_add(1)
            .context("manifest generation overflows")?;
        let physical_len = self
            .pack
            .metadata()
            .context("stat prepared append pack")?
            .len();
        ensure!(
            physical_len == prepared.receipt.frame_end,
            "prepared frame is not the physical pack tail"
        );
        let expected_frame_start = self
            .last_frame_receipt
            .map_or(0, |receipt| receipt.frame_end);
        ensure!(
            prepared.receipt.frame_start == expected_frame_start,
            "prepared frame does not continue the committed pack tail"
        );
        let actual_receipt = read_frame_receipt_at(
            &self.pack,
            prepared.receipt.epoch,
            prepared.receipt.frame_start,
            prepared.receipt.frame_end,
        )?;
        ensure!(
            actual_receipt == prepared.receipt,
            "prepared frame receipt no longer matches durable bytes"
        );
        let pack_map = Arc::new(Mmap::map(
            &self.pack,
            prepared.receipt.frame_end,
            &self.pack_path,
        )?);
        let lookup_pack_map = map_random_if_enabled(
            &self.pack,
            prepared.receipt.frame_end,
            &self.pack_path,
            self.options,
        )?
        .map(Arc::new);
        verify_tail_frame(
            &pack_map,
            prepared.receipt.frame_start,
            prepared.receipt.frame_end,
            prepared.receipt.epoch,
        )?;

        let prepared_run = &pending.run.run;
        let run_path = self.runs_dir.join(run_file_name(
            0,
            prepared.receipt.epoch,
            prepared.receipt.epoch,
        ));
        let verified_run = read_index_run_with_options(&run_path, self.options)?;
        verify_tail_run(&verified_run)?;
        ensure!(
            verified_run.epoch == prepared_run.epoch
                && verified_run.record_count == prepared_run.record_count
                && verified_run.records_sha256 == prepared_run.records_sha256
                && verified_run.file_bytes == prepared_run.file_bytes
                && verified_run.min_key == prepared_run.min_key
                && verified_run.max_key == prepared_run.max_key
                && verified_run.memory_bytes == prepared_run.memory_bytes,
            "prepared index run no longer matches its durable receipt"
        );
        let live_run = LiveRun {
            run: Arc::new(verified_run),
            level: 0,
            min_epoch: prepared.receipt.epoch,
            max_epoch: prepared.receipt.epoch,
        };
        let mut activated_runs = self.runs.clone();
        activated_runs.push(live_run);
        let mut activated_ranges = self.ranges.clone();
        activated_ranges.push(RunRange {
            min_prefix: activated_runs.last().expect("appended run").run.min_prefix,
            max_prefix: activated_runs.last().expect("appended run").run.max_prefix,
        });
        let entries = activated_runs.iter().map(manifest_entry_of).collect();
        Ok(ValidatedAppend {
            receipt: prepared.receipt,
            pack_map,
            lookup_pack_map,
            runs: activated_runs,
            ranges: activated_ranges,
            decoded_index_bytes: pending.decoded_index_bytes,
            next_epoch,
            generation,
            manifest: Manifest {
                generation,
                entries,
            },
        })
    }

    /// Exposes a generation after its manifest is durably published. This
    /// method is intentionally infallible so a successful seal cannot leave
    /// disk publication ahead of the writer's in-process bookkeeping.
    fn install_validated_append(&mut self, validated: ValidatedAppend) {
        self.pack_map = validated.pack_map;
        self.lookup_pack_map = validated.lookup_pack_map;
        self.runs = validated.runs;
        *self.level_run_counts.entry(0).or_default() += 1;
        self.ranges = validated.ranges;
        self.decoded_index_bytes = validated.decoded_index_bytes;
        self.next_epoch = validated.next_epoch;
        self.generation = validated.generation;
        self.last_frame_receipt = Some(validated.receipt);
        self.pending_append = None;
        self.note_peak();
    }

    /// Runs derived index maintenance after no prepared append remains.
    ///
    /// Coordinated callers invoke this only after the external marker commits
    /// and the prepared frame becomes visible. A maintenance failure cannot
    /// roll back that committed frame; callers may drop the writer and let
    /// startup recovery rebuild the derived indexes from the marker horizon.
    pub fn maintain(&mut self) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot maintain index runs while an append awaits activation"
        );
        while let Some(plan) = self.plan_compaction()? {
            self.adopt_compaction(plan.build()?)?;
        }
        Ok(())
    }

    /// Reports bounded derived-index debt without performing I/O.
    pub fn compaction_debt(&self) -> CompactionDebt {
        let mut excess_runs = 0usize;
        let mut backpressure_required = false;
        for (&level, &count) in &self.level_run_counts {
            let bound = self.level_run_bound(level);
            excess_runs = excess_runs.saturating_add(count.saturating_sub(bound));
            backpressure_required |= count >= bound.saturating_add(self.compaction.fanout);
        }
        CompactionDebt {
            live_runs: u64::try_from(self.runs.len()).unwrap_or(u64::MAX),
            excess_runs: u64::try_from(excess_runs).unwrap_or(u64::MAX),
            decoded_index_bytes: self.decoded_index_bytes,
            max_index_memory_bytes: self.max_index_memory_bytes,
            backpressure_required,
        }
    }

    /// Selects and leases the oldest inputs of the first overfull level.
    /// Selection and snapshot pinning are short; [`PackCompactionPlan::build`]
    /// may then run on another thread without holding the pack writer lock.
    pub fn plan_compaction(&self) -> Result<Option<PackCompactionPlan>> {
        let Some(level) = self.first_overfull_level() else {
            return Ok(None);
        };
        self.plan_compaction_at_level(level)
    }

    /// Adopts a previously built derived run into the latest live view and
    /// durably publishes a new manifest. Appends that landed while the output
    /// was built remain in the manifest; only the exact leased inputs leave.
    pub fn adopt_compaction(&mut self, prepared: PreparedPackCompaction) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot adopt compaction while an append awaits activation"
        );
        let adoption_started = Instant::now();
        let pending = prepared.pending;
        ensure!(
            pending.inputs.iter().all(|input| self
                .runs
                .iter()
                .any(|current| same_live_run(current, input))),
            "compaction inputs are no longer part of the live generation"
        );
        let decoded_index_bytes = self
            .decoded_index_bytes
            .checked_sub(pending.input_memory_bytes)
            .and_then(|bytes| bytes.checked_add(pending.run.memory_bytes))
            .context("decoded index bytes overflow")?;
        ensure!(
            decoded_index_bytes <= self.max_index_memory_bytes,
            "compaction output exceeds configured index memory bound"
        );
        self.runs.retain(|current| {
            !pending
                .inputs
                .iter()
                .any(|input| same_live_run(current, input))
        });
        let file_bytes = pending.run.file_bytes;
        self.runs.push(LiveRun {
            run: Arc::new(pending.run),
            level: pending.level,
            min_epoch: pending.min_epoch,
            max_epoch: pending.max_epoch,
        });
        for input in &pending.inputs {
            let count = self
                .level_run_counts
                .get_mut(&input.level)
                .expect("validated compaction input level count");
            *count -= 1;
            if *count == 0 {
                self.level_run_counts.remove(&input.level);
            }
        }
        *self.level_run_counts.entry(pending.level).or_default() += 1;
        self.runs.sort_by_key(|live| live.min_epoch);
        self.decoded_index_bytes = decoded_index_bytes;
        self.rebuild_ranges();
        self.publish_manifest()?;
        self.stats.cycles = self.stats.cycles.saturating_add(1);
        self.stats.runs_merged = self.stats.runs_merged.saturating_add(pending.input_runs);
        self.stats.runs_produced = self.stats.runs_produced.saturating_add(1);
        self.stats.input_records = self
            .stats
            .input_records
            .saturating_add(pending.input_records);
        self.stats.output_records = self
            .stats
            .output_records
            .saturating_add(pending.output_records);
        self.stats.bytes_written = self.stats.bytes_written.saturating_add(file_bytes);
        self.stats.wall_ns = self.stats.wall_ns.saturating_add(
            pending
                .wall_ns
                .saturating_add(duration_ns(adoption_started.elapsed())),
        );
        self.note_peak();
        Ok(())
    }

    /// Atomically republishes the unchanged live run set in the current
    /// manifest format.
    ///
    /// Offline migration tooling uses this after fully validating a legacy
    /// payload prefix and before publishing a new external identity. No frame,
    /// index record, or read-visible value changes.
    pub fn republish_manifest(&mut self) -> Result<()> {
        ensure!(
            self.pending_append.is_none(),
            "cannot republish a manifest while an append awaits activation"
        );
        ensure!(
            self.last_frame_receipt.is_some(),
            "cannot republish a manifest for an empty pack"
        );
        self.publish_manifest()
    }

    /// Newest-committed-version point read.
    pub fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.view().get(key)
    }

    /// Filter-assisted k-way merge: per-run cursors gallop forward over the
    /// sparse fences as the sorted query stream advances, so each run is
    /// visited once per batch instead of once per key binary search.
    /// Filter-assisted k-way batch read. Keys must be sorted ascending;
    /// results align one-to-one with the input order.
    pub fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.view().get_many_sorted(keys)
    }

    fn view(&self) -> ReadView<'_> {
        ReadView {
            runs: &self.runs,
            ranges: &self.ranges,
            pack_map: &self.pack_map,
            lookup_pack_map: self.lookup_pack_map.as_deref(),
        }
    }

    /// Publishes the current live run set as the next manifest generation.
    /// The rename inside `manifest::publish_manifest` is the single atomic
    /// publication point; a crash before it leaves the previous generation
    /// live and orphans only unreferenced run files.
    fn publish_manifest(&mut self) -> Result<()> {
        let generation = self
            .generation
            .checked_add(1)
            .context("manifest generation overflows")?;
        let entries = self.runs.iter().map(manifest_entry_of).collect();
        manifest::publish_manifest(
            &self.root,
            &Manifest {
                generation,
                entries,
            },
        )?;
        self.generation = generation;
        Ok(())
    }

    fn level_run_bound(&self, level: u32) -> usize {
        if level == 0 {
            self.compaction.l0_bound
        } else {
            self.compaction.l1_bound
        }
    }

    fn first_overfull_level(&self) -> Option<u32> {
        self.level_run_counts
            .iter()
            .find_map(|(&level, &count)| (count > self.level_run_bound(level)).then_some(level))
    }

    fn plan_compaction_at_level(&self, level: u32) -> Result<Option<PackCompactionPlan>> {
        ensure!(
            level < MAX_COMPACTION_LEVEL,
            "derived index exceeded the maximum compaction level"
        );
        let source_snapshot = self.snapshot()?;
        let inputs: Vec<LiveRun> = source_snapshot
            .runs
            .iter()
            .filter(|live| live.level == level)
            .take(self.compaction.fanout)
            .cloned()
            .collect();
        if inputs.len() < 2 {
            return Ok(None);
        }
        Ok(Some(PackCompactionPlan {
            level,
            inputs,
            runs_dir: self.runs_dir.clone(),
            random_point_mmap: self.options.random_point_mmap,
            _source_snapshot: source_snapshot,
        }))
    }

    /// Merges the oldest runs of one level (up to the fanout) into one run
    /// at the next level: records are decoded, checksum-scrubbed, merged
    /// newest-epoch-wins, and re-encoded with rebuilt fences and filter.
    /// The output is an ordinary v3 run file whose payload offsets keep
    /// pointing at the original frames. Nothing in the live set changes and
    /// no manifest is published here, so calling this without adopting the
    /// result exactly simulates a crash after run-file publication.
    #[cfg(test)]
    fn build_compacted_run(&self, level: u32) -> Result<Option<PendingMerge>> {
        let Some(plan) = self.plan_compaction_at_level(level)? else {
            return Ok(None);
        };
        Ok(Some(plan.build()?.pending))
    }

    /// Pins the current manifest generation: the snapshot keeps its own run
    /// references and pack mapping, and the lease blocks reclamation of the
    /// generation's run files until the snapshot is dropped.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.pin_snapshot_parts(
            self.generation,
            &self.runs,
            &self.ranges,
            &self.pack_map,
            self.lookup_pack_map.as_ref(),
        )
    }

    /// Pins an immutable generation assembled either from the current view or
    /// by pre-seal validation before its manifest publication.
    fn pin_snapshot_parts(
        &self,
        generation: u64,
        runs: &[LiveRun],
        ranges: &[RunRange],
        pack_map: &Arc<Mmap>,
        lookup_pack_map: Option<&Arc<Mmap>>,
    ) -> Result<Snapshot> {
        {
            let mut leases = self
                .leases
                .lock()
                .map_err(|error| anyhow::anyhow!("snapshot lease book is poisoned: {error}"))?;
            *leases.entry(generation).or_insert(0) += 1;
        }
        Ok(Snapshot {
            generation,
            runs: runs.to_vec(),
            ranges: ranges.to_vec(),
            pack_map: Arc::clone(pack_map),
            lookup_pack_map: lookup_pack_map.map(Arc::clone),
            leases: Arc::clone(&self.leases),
        })
    }

    /// Explicit reclamation of superseded runs and manifests. The current
    /// generation and every leased generation stay untouched; everything
    /// else (superseded manifests, their run files, orphan runs from
    /// interrupted appends or compactions, stale temp files) is deleted.
    /// Never called implicitly during reads.
    pub fn gc(&mut self) -> Result<GcStats> {
        ensure!(
            self.pending_append.is_none(),
            "cannot reclaim files while an append awaits activation"
        );
        let mut protected_generations: Vec<u64> = {
            let leases = self
                .leases
                .lock()
                .map_err(|error| anyhow::anyhow!("snapshot lease book is poisoned: {error}"))?;
            leases.keys().copied().collect()
        };
        if self.generation > 0 && !protected_generations.contains(&self.generation) {
            protected_generations.push(self.generation);
        }
        let mut protected_runs: HashSet<String> = HashSet::new();
        for generation in &protected_generations {
            let path = self.root.join(manifest::manifest_file_name(*generation));
            let manifest = manifest::read_manifest(&path)
                .with_context(|| format!("load protected manifest generation {generation}"))?;
            protected_runs.extend(manifest.entries.into_iter().map(|entry| entry.file_name));
        }
        let mut stats = GcStats::default();
        for entry in fs::read_dir(&self.runs_dir)
            .with_context(|| format!("read index-run directory {}", self.runs_dir.display()))?
        {
            let entry = entry.context("read index-run directory entry")?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default();
            let delete = match extension {
                "tmp" => true,
                "idx" => !protected_runs.contains(name),
                _ => false,
            };
            if !delete {
                continue;
            }
            let bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            fs::remove_file(&path)
                .with_context(|| format!("delete superseded index run {}", path.display()))?;
            stats.bytes_reclaimed = stats.bytes_reclaimed.saturating_add(bytes);
            if extension == "idx" {
                stats.runs_deleted = stats.runs_deleted.saturating_add(1);
            }
        }
        for (generation, path) in manifest::list_manifest_files(&self.root)? {
            if protected_generations.contains(&generation) {
                continue;
            }
            let bytes = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            fs::remove_file(&path)
                .with_context(|| format!("delete superseded manifest {}", path.display()))?;
            stats.manifests_deleted = stats.manifests_deleted.saturating_add(1);
            stats.bytes_reclaimed = stats.bytes_reclaimed.saturating_add(bytes);
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read pack store directory {}", self.root.display()))?
        {
            let entry = entry.context("read pack store directory entry")?;
            let path = entry.path();
            let is_manifest_tmp = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("manifest-") && name.ends_with(".tmp"));
            if is_manifest_tmp {
                fs::remove_file(&path)
                    .with_context(|| format!("delete stale manifest temp {}", path.display()))?;
            }
        }
        sync_directory(&self.runs_dir)?;
        sync_directory(&self.root)?;
        self.stats.gc_cycles = self.stats.gc_cycles.saturating_add(1);
        self.stats.gc_runs_deleted = self
            .stats
            .gc_runs_deleted
            .saturating_add(stats.runs_deleted);
        self.stats.gc_manifests_deleted = self
            .stats
            .gc_manifests_deleted
            .saturating_add(stats.manifests_deleted);
        self.stats.gc_bytes_reclaimed = self
            .stats
            .gc_bytes_reclaimed
            .saturating_add(stats.bytes_reclaimed);
        Ok(stats)
    }

    /// Physical layout: (pack bytes, live index bytes, live run count,
    /// decoded index memory bytes).
    pub fn layout(&self) -> Result<(u64, u64, u64, u64)> {
        let pack_bytes = self.pack.metadata().context("stat append pack")?.len();
        let index_bytes = self.runs.iter().try_fold(0u64, |total, live| {
            total
                .checked_add(live.run.file_bytes)
                .context("index bytes overflow")
        })?;
        Ok((
            pack_bytes,
            index_bytes,
            u64::try_from(self.runs.len()).context("run count does not fit u64")?,
            self.decoded_index_bytes,
        ))
    }

    /// Structural counts observed when this handle opened the store.
    pub const fn open_validation(&self) -> OpenValidation {
        self.open_validation
    }

    /// Cumulative compaction and reclamation evidence for this store.
    pub const fn compaction_stats(&self) -> CompactionStats {
        self.stats
    }

    /// Placement and checksum of the newest visible frame. A prepared frame
    /// does not replace this receipt until activation publishes its manifest.
    pub const fn last_frame_receipt(&self) -> Option<PackFrameReceipt> {
        self.last_frame_receipt
    }

    fn rebuild_ranges(&mut self) {
        self.ranges = self
            .runs
            .iter()
            .map(|live| RunRange {
                min_prefix: live.run.min_prefix,
                max_prefix: live.run.max_prefix,
            })
            .collect();
    }

    fn note_peak(&mut self) {
        debug_assert_eq!(
            self.level_run_counts.values().sum::<usize>(),
            self.runs.len(),
            "per-level run directory diverged from the live manifest"
        );
        self.stats.peak_live_runs = self
            .stats
            .peak_live_runs
            .max(u64::try_from(self.runs.len()).unwrap_or(u64::MAX));
    }

    /// Re-hashes and structurally decodes every committed frame.
    ///
    /// Normal open verifies frame headers, all derived-run checksums, and the
    /// committed tail payload. This slower operation is the explicit migration
    /// and offline-scrub gate for proving the complete payload prefix rather
    /// than only the tail.
    pub fn scrub_committed_frames(&self) -> Result<PackScrubStats> {
        self.scrub_committed_frames_with(|_, _, _| Ok(()))
    }

    /// Scrubs and hashes a complete ordered, unique, put-only checkpoint.
    ///
    /// This deliberately rejects runtime version streams containing repeated
    /// keys or tombstones. It proves that checkpoint frame rows reproduce the
    /// same canonical namespace stream hashed by the offline builder.
    pub fn scrub_checkpoint_namespace(&self) -> Result<CheckpointNamespaceEvidence> {
        let mut hasher = Sha256::new();
        hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        let mut previous_key = None;
        let scrub = self.scrub_committed_frames_with(|key, kind, value| {
            ensure!(kind == 1, "checkpoint namespace contains a tombstone");
            if let Some(previous) = previous_key {
                ensure!(
                    previous < *key,
                    "checkpoint namespace keys are not strictly increasing"
                );
            }
            hasher.update((PACK_KEY_BYTES as u32).to_le_bytes());
            hasher.update(key);
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
            previous_key = Some(*key);
            Ok(())
        })?;
        Ok(CheckpointNamespaceEvidence {
            scrub,
            sha256: hasher.finalize().into(),
        })
    }

    fn scrub_committed_frames_with<F>(&self, mut visit: F) -> Result<PackScrubStats>
    where
        F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
    {
        let bytes = self.pack_map.as_slice();
        let mut stats = PackScrubStats::default();
        let mut offset = 0usize;
        let expected_frames = self
            .last_frame_receipt
            .map_or(0, |receipt| receipt.epoch.saturating_add(1));

        while stats.frames < expected_frames {
            let header_end = offset
                .checked_add(FRAME_HEADER_LEN)
                .context("scrub frame header offset overflows")?;
            let header: &[u8; FRAME_HEADER_LEN] = bytes
                .get(offset..header_end)
                .with_context(|| format!("committed frame {} header is truncated", stats.frames))?
                .try_into()
                .expect("frame header length");
            let payload_len = validate_frame_header(header, stats.frames)?;
            let payload_len =
                usize::try_from(payload_len).context("scrub payload length does not fit usize")?;
            let payload_end = header_end
                .checked_add(payload_len)
                .context("scrub frame end offset overflows")?;
            let payload = bytes.get(header_end..payload_end).with_context(|| {
                format!("committed frame {} payload is truncated", stats.frames)
            })?;
            ensure!(
                digest(payload).as_slice() == &header[40..72],
                "committed frame {} payload checksum mismatch",
                stats.frames
            );
            let expected_rows = usize::try_from(u64_at(header, 24)?)
                .context("scrub row count does not fit usize")?;
            let payload_stats = validate_payload_rows_with(payload, expected_rows, &mut visit)?;
            stats.frames = stats.frames.saturating_add(1);
            stats.rows = stats.rows.saturating_add(payload_stats.rows);
            stats.puts = stats.puts.saturating_add(payload_stats.puts);
            stats.tombstones = stats.tombstones.saturating_add(payload_stats.tombstones);
            stats.payload_bytes = stats
                .payload_bytes
                .saturating_add(u64::try_from(payload.len()).expect("payload length fits u64"));
            stats.value_bytes = stats.value_bytes.saturating_add(payload_stats.value_bytes);
            offset = payload_end;
        }

        ensure!(
            offset == bytes.len(),
            "committed frame prefix ends at {offset}, but mapped pack has {} bytes",
            bytes.len()
        );
        Ok(stats)
    }
}

/// Shared newest-first read path over one pinned run set; used by the live
/// store and by snapshot generations alike.
struct ReadView<'a> {
    runs: &'a [LiveRun],
    ranges: &'a [RunRange],
    pack_map: &'a Mmap,
    lookup_pack_map: Option<&'a Mmap>,
}

impl ReadView<'_> {
    fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.lookup(key, None)
    }

    fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        ensure!(
            keys.windows(2).all(|pair| pair[0] <= pair[1]),
            "batch keys must be sorted"
        );
        let mut cursors = vec![0usize; self.runs.len()];
        let mut results = vec![None; keys.len()];
        let mut values = Vec::new();
        for (output_index, key) in keys.iter().enumerate() {
            let Some(entry) = self.lookup_entry(key, Some(&mut cursors))? else {
                continue;
            };
            if !entry.tombstone {
                values.push((output_index, entry));
            }
        }

        // Hash-sorted node keys have no useful relationship with append
        // offsets. Reordering derived locations still reduces seeks and makes
        // duplicate locations adjacent, but hits remain sparse across the
        // complete pack and use the random-advised payload mapping.
        values.sort_unstable_by_key(|(_, entry)| entry.value_offset);
        let mut previous: Option<(u64, u32, Vec<u8>)> = None;
        for (output_index, entry) in values {
            let value = match previous.as_ref() {
                Some((offset, length, value))
                    if *offset == entry.value_offset && *length == entry.value_len =>
                {
                    value.clone()
                }
                _ => {
                    let value = self
                        .entry_value(entry)?
                        .expect("non-tombstone index entry has a value");
                    previous = Some((entry.value_offset, entry.value_len, value.clone()));
                    value
                }
            };
            results[output_index] = Some(value);
        }
        Ok(results)
    }

    /// Newest-first verified lookup: the compact range directory rejects
    /// out-of-range runs with two integer compares, then the per-run xor
    /// filter proves absence without any positioned read, then the sparse
    /// fences locate the single record window that a positioned read probes.
    fn lookup(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        cursors: Option<&mut [usize]>,
    ) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.lookup_entry(key, cursors)? else {
            return Ok(None);
        };
        self.entry_value(entry)
    }

    fn lookup_entry(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        mut cursors: Option<&mut [usize]>,
    ) -> Result<Option<IndexEntry>> {
        let prefix = key_prefix(key);
        let mut cached_hash = None;
        for index in (0..self.runs.len()).rev() {
            let range = &self.ranges[index];
            if prefix < range.min_prefix || prefix > range.max_prefix {
                continue;
            }
            let run = &self.runs[index].run;
            // The full 33-byte boundary check runs only on a leading-u64 tie.
            if (prefix == range.min_prefix && *key < run.min_key)
                || (prefix == range.max_prefix && *key > run.max_key)
            {
                continue;
            }
            let hash = *cached_hash.get_or_insert_with(|| key_hash(key));
            let cursor = cursors.as_deref_mut().map(|cursors| &mut cursors[index]);
            let Some(entry) = run.probe_membership(key, hash, cursor)? else {
                continue;
            };
            return Ok(Some(entry));
        }
        Ok(None)
    }

    fn entry_value(&self, entry: IndexEntry) -> Result<Option<Vec<u8>>> {
        if entry.tombstone {
            return Ok(None);
        }
        let offset =
            usize::try_from(entry.value_offset).context("value offset does not fit usize")?;
        let length = usize::try_from(entry.value_len).context("value length does not fit usize")?;
        let end = offset
            .checked_add(length)
            .context("value end offset overflows")?;
        let pack_map = self.lookup_pack_map.unwrap_or(self.pack_map);
        let value = pack_map
            .as_slice()
            .get(offset..end)
            .context("indexed value outside the append pack")?;
        Ok(Some(value.to_vec()))
    }
}

/// A read snapshot pinning one manifest generation. Run references and the
/// pack mapping are held directly, so reads stay valid even if compaction
/// replaces the live set; the lease additionally keeps the generation's run
/// files on disk until the snapshot is dropped and `gc` runs.
pub struct Snapshot {
    generation: u64,
    runs: Vec<LiveRun>,
    ranges: Vec<RunRange>,
    pack_map: Arc<Mmap>,
    lookup_pack_map: Option<Arc<Mmap>>,
    leases: Arc<Mutex<BTreeMap<u64, usize>>>,
}

impl Snapshot {
    /// The pinned manifest generation.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Newest-committed-version point read.
    pub fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.view().get(key)
    }

    /// Filter-assisted k-way batch read. Keys must be sorted ascending;
    /// results align one-to-one with the input order.
    pub fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.view().get_many_sorted(keys)
    }

    fn view(&self) -> ReadView<'_> {
        ReadView {
            runs: &self.runs,
            ranges: &self.ranges,
            pack_map: &self.pack_map,
            lookup_pack_map: self.lookup_pack_map.as_deref(),
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if let Ok(mut leases) = self.leases.lock() {
            if let Some(count) = leases.get_mut(&self.generation) {
                *count -= 1;
                if *count == 0 {
                    leases.remove(&self.generation);
                }
            }
        }
    }
}

impl IndexRun {
    /// Filter gate plus fence-guided record probe. The caller has already
    /// proven the key is inside this run's key range.
    fn probe_membership(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        hash: u64,
        cursor: Option<&mut usize>,
    ) -> Result<Option<IndexEntry>> {
        if !self.filter.maybe_contains_hash(hash) {
            return Ok(None);
        }
        self.probe(key, cursor)
    }

    /// Fence binary search (or gallop from the batch cursor) plus one
    /// in-memory search of the covering mapped record window.
    fn probe(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        cursor: Option<&mut usize>,
    ) -> Result<Option<IndexEntry>> {
        let truncated = truncate_key(key);
        let partition = match cursor {
            Some(hint) => {
                let partition = gallop_partition_point(&self.fences, &truncated, *hint);
                *hint = partition;
                partition
            }
            None => self.fences.partition_point(|fence| fence < &truncated),
        };
        let fence_end =
            partition + self.fences[partition..].partition_point(|fence| fence <= &truncated);
        let record_count =
            usize::try_from(self.record_count).context("record count does not fit usize")?;
        let first = (partition.saturating_sub(1) * FENCE_INTERVAL).min(record_count);
        let last = (fence_end * FENCE_INTERVAL).min(record_count);
        if first >= last {
            return Ok(None);
        }
        let records_start =
            usize::try_from(self.records_offset).context("records offset does not fit usize")?;
        // Both point and sorted-batch probes touch sparse record windows.
        // Sorted cursors are monotone but still jump across a very large run;
        // using the ordinary map here caused multi-megabyte readahead per
        // verified hit. Sequential verification and compaction access `map`
        // directly and retain their ordinary readahead policy.
        let map = self.lookup_map.as_ref().unwrap_or(&self.map);
        let window = map
            .as_slice()
            .get(records_start + first * INDEX_RECORD_LEN..records_start + last * INDEX_RECORD_LEN)
            .context("fence probe window outside the run")?;
        let mut low = 0usize;
        let mut high = window.len() / INDEX_RECORD_LEN;
        while low < high {
            let mid = low + (high - low) / 2;
            let record_key: &[u8; PACK_KEY_BYTES] = window
                [mid * INDEX_RECORD_LEN..mid * INDEX_RECORD_LEN + PACK_KEY_BYTES]
                .try_into()
                .expect("record key");
            if record_key <= key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == 0 {
            return Ok(None);
        }
        let entry = decode_record(&window[(low - 1) * INDEX_RECORD_LEN..low * INDEX_RECORD_LEN])?;
        if &entry.key != key {
            return Ok(None);
        }
        Ok(Some(entry))
    }
}

/// Galloping partition point over sorted fences. `hint` must not exceed the
/// true partition point (monotone sorted batch queries guarantee this).
fn gallop_partition_point(
    fences: &[[u8; FENCE_KEY_BYTES]],
    target: &[u8; FENCE_KEY_BYTES],
    hint: usize,
) -> usize {
    debug_assert!(hint <= fences.len());
    let mut lower = hint;
    let mut step = 1usize;
    while lower + step < fences.len() && fences[lower + step] < *target {
        lower += step;
        step <<= 1;
    }
    let upper = (lower + step).min(fences.len());
    lower + fences[lower..upper].partition_point(|fence| fence < target)
}

fn truncate_key(key: &[u8; PACK_KEY_BYTES]) -> [u8; FENCE_KEY_BYTES] {
    key[..FENCE_KEY_BYTES].try_into().expect("fence key prefix")
}

/// Big-endian leading u64 of a key, order-equivalent to byte comparison.
fn key_prefix(key: &[u8; PACK_KEY_BYTES]) -> u64 {
    u64::from_be_bytes(key[..8].try_into().expect("key prefix"))
}

fn distinct_keys(entries: &[IndexEntry]) -> Vec<[u8; PACK_KEY_BYTES]> {
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        if keys.last() != Some(&entry.key) {
            keys.push(entry.key);
        }
    }
    keys
}

fn build_fences(entries: &[IndexEntry]) -> Vec<[u8; FENCE_KEY_BYTES]> {
    (0..entries.len())
        .step_by(FENCE_INTERVAL)
        .map(|start| truncate_key(&entries[start].key))
        .collect()
}

fn filter_seed(epoch: u64) -> u64 {
    epoch
        .wrapping_mul(0xA076_1D64_78BD_642F)
        .wrapping_add(0xE703_7ED1_A0B4_28DB)
}

/// Resident structured bytes for one run: fences, filter, and metadata.
/// Index records are never decoded into memory, so they are not charged.
fn run_structured_bytes(records: usize, distinct: usize) -> Result<u64> {
    let fences = u64::try_from(records.div_ceil(FENCE_INTERVAL) * FENCE_KEY_BYTES)
        .context("fence bytes do not fit u64")?;
    let filter =
        u64::try_from(filter_capacity(distinct) * 2).context("filter bytes do not fit u64")?;
    fences
        .checked_add(filter)
        .and_then(|total| total.checked_add(RUN_METADATA_BYTES))
        .context("structured index bytes overflow")
}

fn decode_record(record: &[u8]) -> Result<IndexEntry> {
    ensure!(record.len() == INDEX_RECORD_LEN, "short index record");
    let mut key = [0u8; PACK_KEY_BYTES];
    key.copy_from_slice(&record[..PACK_KEY_BYTES]);
    Ok(IndexEntry {
        key,
        sequence: u32_at(record, PACK_KEY_BYTES)?,
        value_offset: u64_at(record, PACK_KEY_BYTES + 4)?,
        value_len: u32_at(record, PACK_KEY_BYTES + 12)?,
        tombstone: record[PACK_KEY_BYTES + 16] != 0,
    })
}

fn encode_frame_payload(
    frame_start: u64,
    operations: &[PackOperation],
) -> Result<(Vec<u8>, Vec<IndexEntry>)> {
    ensure!(
        !operations.is_empty(),
        "frame must contain at least one row"
    );
    let operation_count =
        u64::try_from(operations.len()).context("frame row count overflows u64")?;
    ensure!(
        operation_count <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let estimated = operations.iter().try_fold(0usize, |total, operation| {
        let value_len = match &operation.kind {
            PackOpKind::Put(value) => value.len(),
            PackOpKind::Tombstone => 0,
        };
        total
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .and_then(|total| total.checked_add(value_len))
            .context("frame payload size overflows usize")
    })?;
    let estimated_u64 = u64::try_from(estimated).context("frame payload size overflows u64")?;
    ensure!(
        estimated_u64 <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let mut payload = Vec::with_capacity(estimated);
    let mut entries = Vec::with_capacity(operations.len());
    for (sequence, operation) in operations.iter().enumerate() {
        payload.extend_from_slice(&operation.key);
        let value_start = payload
            .len()
            .checked_add(1 + 4)
            .context("frame value offset overflows usize")?;
        let (tombstone, value) = match &operation.kind {
            PackOpKind::Put(value) => (false, value.as_slice()),
            PackOpKind::Tombstone => (true, &[][..]),
        };
        payload.push(u8::from(!tombstone));
        let value_len = u32::try_from(value.len()).context("frame value exceeds u32")?;
        payload.extend_from_slice(&value_len.to_le_bytes());
        payload.extend_from_slice(value);
        let value_offset = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|offset| offset.checked_add(value_start as u64))
            .context("absolute frame value offset overflows u64")?;
        entries.push(IndexEntry {
            key: operation.key,
            sequence: u32::try_from(sequence).context("frame sequence exceeds u32")?,
            value_offset,
            value_len,
            tombstone,
        });
    }
    ensure!(
        payload.len() == estimated,
        "encoded frame length changed unexpectedly"
    );
    Ok((payload, entries))
}

/// Reconstructs one frame's index entries from its payload rows. Used only
/// by the rebuild path; offsets point at the original payload bytes.
fn decode_frame_payload(frame_start: u64, payload: &[u8]) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    let mut sequence = 0u32;
    while offset < payload.len() {
        let header_end = offset
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .context("row header offset overflows")?;
        ensure!(header_end <= payload.len(), "truncated frame row header");
        let mut key = [0u8; PACK_KEY_BYTES];
        key.copy_from_slice(&payload[offset..offset + PACK_KEY_BYTES]);
        let kind = payload[offset + PACK_KEY_BYTES];
        ensure!(kind <= 1, "invalid frame row kind");
        let value_len = u32_at(payload, offset + PACK_KEY_BYTES + 1)?;
        ensure!(kind == 1 || value_len == 0, "tombstone carries a value");
        let value_start = header_end;
        let value_end = value_start
            .checked_add(value_len as usize)
            .context("row value offset overflows")?;
        ensure!(value_end <= payload.len(), "truncated frame row value");
        let value_offset = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|offset| offset.checked_add(value_start as u64))
            .context("absolute frame value offset overflows u64")?;
        entries.push(IndexEntry {
            key,
            sequence,
            value_offset,
            value_len,
            tombstone: kind == 0,
        });
        sequence = sequence
            .checked_add(1)
            .context("frame sequence exceeds u32")?;
        offset = value_end;
    }
    Ok(entries)
}

fn encode_frame_header(
    epoch: u64,
    rows: usize,
    payload_len: usize,
    checksum: [u8; 32],
) -> Result<[u8; FRAME_HEADER_LEN]> {
    ensure!(rows > 0, "frame must contain at least one row");
    let rows_u64 = u64::try_from(rows).context("frame row count does not fit u64")?;
    ensure!(
        rows_u64 <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let payload_len_u64 =
        u64::try_from(payload_len).context("frame payload length does not fit u64")?;
    ensure!(
        payload_len_u64 <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let minimum_payload = rows_u64
        .checked_mul(FRAME_ROW_HEADER_BYTES)
        .context("minimum frame payload length overflows")?;
    ensure!(
        payload_len_u64 >= minimum_payload,
        "frame payload is too short for its declared row count"
    );
    let mut bytes = [0u8; FRAME_HEADER_LEN];
    bytes[0..8].copy_from_slice(FRAME_MAGIC);
    bytes[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
    bytes[24..32].copy_from_slice(&rows_u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&payload_len_u64.to_le_bytes());
    bytes[40..72].copy_from_slice(&checksum);
    Ok(bytes)
}

/// Validates a frame header and returns its payload length.
fn validate_frame_header(header: &[u8; FRAME_HEADER_LEN], expected_epoch: u64) -> Result<u64> {
    ensure!(&header[0..8] == FRAME_MAGIC, "invalid frame magic");
    ensure!(
        u32_at(header, 8)? == PACK_FRAME_FORMAT_VERSION,
        "unsupported frame version"
    );
    ensure!(
        u32_at(header, 12)? as usize == FRAME_HEADER_LEN,
        "invalid frame header length"
    );
    ensure!(
        u64_at(header, 16)? == expected_epoch,
        "non-contiguous frame epoch"
    );
    let rows = u64_at(header, 24)?;
    ensure!(rows > 0, "frame must contain at least one row");
    ensure!(
        rows <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let payload_len = u64_at(header, 32)?;
    ensure!(
        payload_len <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let minimum_payload = rows
        .checked_mul(FRAME_ROW_HEADER_BYTES)
        .context("minimum frame payload length overflows")?;
    ensure!(
        payload_len >= minimum_payload,
        "frame payload is too short for its declared row count"
    );
    Ok(payload_len)
}

/// Validated frame end offsets. Anything beyond the last complete,
/// well-formed frame is a torn or orphaned tail handled by the caller.
struct FrameScan {
    frame_ends: Vec<u64>,
}

/// Walks frame headers without reading payloads; stops at the first torn or
/// malformed frame start so the caller can truncate the uncommitted tail.
fn scan_frames(pack: &File) -> Result<FrameScan> {
    let file_len = pack.metadata().context("stat append pack")?.len();
    let mut frame_ends = Vec::new();
    let mut offset = 0u64;
    let mut expected_epoch = 0u64;
    while offset < file_len {
        let mut header = [0u8; FRAME_HEADER_LEN];
        let mut filled = 0usize;
        while filled < FRAME_HEADER_LEN {
            let read = pack
                .read_at(&mut header[filled..], offset + filled as u64)
                .context("read frame header")?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled < FRAME_HEADER_LEN {
            break;
        }
        let Ok(payload_len) = validate_frame_header(&header, expected_epoch) else {
            break;
        };
        let frame_end = offset
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_len))
            .context("frame end offset overflows")?;
        if frame_end > file_len {
            break;
        }
        frame_ends.push(frame_end);
        offset = frame_end;
        expected_epoch = expected_epoch
            .checked_add(1)
            .context("frame epoch overflows")?;
    }
    Ok(FrameScan { frame_ends })
}

fn read_frame_receipt(pack: &File, scan: &FrameScan, epoch: u64) -> Result<PackFrameReceipt> {
    let index = usize::try_from(epoch).context("frame epoch does not fit usize")?;
    let frame_end = *scan
        .frame_ends
        .get(index)
        .with_context(|| format!("frame {epoch} is not present in the append pack"))?;
    let frame_start = if index == 0 {
        0
    } else {
        scan.frame_ends[index - 1]
    };
    read_frame_receipt_at(pack, epoch, frame_start, frame_end)
}

fn read_frame_receipt_at(
    pack: &File,
    epoch: u64,
    frame_start: u64,
    frame_end: u64,
) -> Result<PackFrameReceipt> {
    let mut header = [0u8; FRAME_HEADER_LEN];
    pack.read_exact_at(&mut header, frame_start)
        .with_context(|| format!("read frame {epoch} header"))?;
    let payload_bytes = validate_frame_header(&header, epoch)?;
    ensure!(
        frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_bytes))
            == Some(frame_end),
        "frame {epoch} length does not match the validated frame chain"
    );
    Ok(PackFrameReceipt {
        epoch,
        frame_start,
        frame_end,
        rows: u64_at(&header, 24)?,
        payload_bytes,
        payload_sha256: header[40..72].try_into().expect("frame payload checksum"),
    })
}

/// Discards derived visibility state and truncates the payload stream to the
/// canonical marker. This runs only during startup, before snapshots or the
/// single writer exist, so no manifest lease can observe the reset.
fn reset_derived_state_to_frame_prefix(
    root: &Path,
    scan: &FrameScan,
    expected_frames: u64,
) -> Result<()> {
    let expected = usize::try_from(expected_frames).context("frame count does not fit usize")?;
    let committed_end = if expected == 0 {
        0
    } else {
        *scan
            .frame_ends
            .get(expected - 1)
            .context("committed frame prefix is incomplete")?
    };
    let pack_path = root.join("frames.pack");
    let pack = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pack_path)
        .with_context(|| format!("open append pack {} for recovery", pack_path.display()))?;
    if pack
        .metadata()
        .context("stat append pack for recovery")?
        .len()
        != committed_end
    {
        pack.set_len(committed_end)
            .context("truncate append pack to canonical marker")?;
        pack.sync_data()
            .context("sync marker-truncated append pack")?;
    }
    drop(pack);

    let runs_dir = root.join("runs");
    for entry in fs::read_dir(&runs_dir)
        .with_context(|| format!("read index-run directory {}", runs_dir.display()))?
    {
        let entry = entry.context("read index-run recovery entry")?;
        let path = entry.path();
        let remove = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "idx" || extension == "tmp");
        if remove {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived index run {}", path.display()))?;
        }
    }
    for entry in fs::read_dir(root)
        .with_context(|| format!("read pack root {} for recovery", root.display()))?
    {
        let entry = entry.context("read pack recovery entry")?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("manifest-") && (name.ends_with(".man") || name.ends_with(".tmp")) {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived manifest {}", path.display()))?;
        }
    }
    sync_directory(&runs_dir)?;
    sync_directory(root)?;
    Ok(())
}

/// Fully verifies the committed tail frame: header, payload checksum, and
/// row structure. This is the only payload read during open.
fn verify_tail_frame(pack: &Mmap, frame_start: u64, frame_end: u64, epoch: u64) -> Result<()> {
    let start = usize::try_from(frame_start).context("tail frame offset does not fit usize")?;
    let header: &[u8; FRAME_HEADER_LEN] = pack
        .as_slice()
        .get(start..start + FRAME_HEADER_LEN)
        .context("read committed tail frame header")?
        .try_into()
        .expect("frame header length");
    let payload_len = validate_frame_header(header, epoch)?;
    let row_count = usize::try_from(u64_at(header, 24)?).context("row count does not fit usize")?;
    ensure!(
        frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_len))
            == Some(frame_end),
        "committed tail frame length mismatch"
    );
    let payload_end = usize::try_from(frame_end).context("tail frame end does not fit usize")?;
    let payload = pack
        .as_slice()
        .get(start + FRAME_HEADER_LEN..payload_end)
        .context("read committed tail frame payload")?;
    ensure!(
        digest(payload).as_slice() == &header[40..72],
        "frame payload checksum mismatch in committed tail frame"
    );
    validate_payload_rows(payload, row_count)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PayloadRowStats {
    rows: u64,
    puts: u64,
    tombstones: u64,
    value_bytes: u64,
}

fn validate_payload_rows(payload: &[u8], expected_rows: usize) -> Result<PayloadRowStats> {
    validate_payload_rows_with(payload, expected_rows, &mut |_, _, _| Ok(()))
}

fn validate_payload_rows_with<F>(
    payload: &[u8],
    expected_rows: usize,
    visit: &mut F,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
{
    let mut offset = 0usize;
    let mut rows = 0usize;
    let mut stats = PayloadRowStats::default();
    while offset < payload.len() {
        let header_end = offset
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .context("row header offset overflows")?;
        ensure!(header_end <= payload.len(), "truncated frame row header");
        let key: &[u8; PACK_KEY_BYTES] = payload[offset..offset + PACK_KEY_BYTES]
            .try_into()
            .expect("validated row key length");
        let kind = payload[offset + PACK_KEY_BYTES];
        ensure!(kind <= 1, "invalid frame row kind");
        let value_len = u32_at(payload, offset + PACK_KEY_BYTES + 1)? as usize;
        ensure!(kind == 1 || value_len == 0, "tombstone carries a value");
        let value_end = header_end
            .checked_add(value_len)
            .context("row value offset overflows")?;
        let value = payload
            .get(header_end..value_end)
            .context("truncated frame row value")?;
        visit(key, kind, value)?;
        if kind == 0 {
            stats.tombstones = stats.tombstones.saturating_add(1);
        } else {
            stats.puts = stats.puts.saturating_add(1);
            stats.value_bytes = stats.value_bytes.saturating_add(
                u64::try_from(value_len).context("row value length does not fit u64")?,
            );
        }
        offset = value_end;
        rows = rows.checked_add(1).context("frame row count overflows")?;
    }
    ensure!(rows == expected_rows, "frame row count mismatch");
    stats.rows = u64::try_from(rows).context("frame row count does not fit u64")?;
    Ok(stats)
}

/// Encodes one immutable sorted run (format v3): header, sparse fences, the
/// xor16 membership filter, then the v1 record section. Everything before
/// the records is derived data rebuilt at publish time. The v3 structure
/// digest binds every lookup-routing header field plus the exact serialized
/// fences and filter before either accelerator may be trusted.
fn encode_index_run(
    epoch: u64,
    entries: &[IndexEntry],
    fences: &[[u8; FENCE_KEY_BYTES]],
    filter: &XorFilter,
    min_key: &[u8; PACK_KEY_BYTES],
    max_key: &[u8; PACK_KEY_BYTES],
) -> Result<(Vec<u8>, [u8; 32])> {
    let record_bytes = entries
        .len()
        .checked_mul(INDEX_RECORD_LEN)
        .context("index run size overflows usize")?;
    let mut records = Vec::with_capacity(record_bytes);
    for entry in entries {
        records.extend_from_slice(&entry.key);
        records.extend_from_slice(&entry.sequence.to_le_bytes());
        records.extend_from_slice(&entry.value_offset.to_le_bytes());
        records.extend_from_slice(&entry.value_len.to_le_bytes());
        records.push(u8::from(entry.tombstone));
    }
    let records_sha256 = digest(&records);
    let mut header = [0u8; INDEX_HEADER_LEN];
    header[0..8].copy_from_slice(INDEX_MAGIC);
    header[8..12].copy_from_slice(&PACK_INDEX_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(INDEX_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&epoch.to_le_bytes());
    header[24..32].copy_from_slice(
        &u64::try_from(entries.len())
            .context("index entry count does not fit u64")?
            .to_le_bytes(),
    );
    header[32..64].copy_from_slice(&records_sha256);
    header[64..68].copy_from_slice(
        &u32::try_from(fences.len())
            .context("fence count does not fit u32")?
            .to_le_bytes(),
    );
    header[68..72].copy_from_slice(&(FENCE_INTERVAL as u32).to_le_bytes());
    header[72..80].copy_from_slice(&filter.seed().to_le_bytes());
    header[80..84].copy_from_slice(
        &u32::try_from(filter.fingerprint_count())
            .context("filter size does not fit u32")?
            .to_le_bytes(),
    );
    header[84..88].copy_from_slice(&FILTER_FINGERPRINT_BITS.to_le_bytes());
    header[88..121].copy_from_slice(min_key);
    header[121..154].copy_from_slice(max_key);
    let filter_bytes = filter.encode();
    let mut output = Vec::with_capacity(
        INDEX_HEADER_LEN + fences.len() * FENCE_KEY_BYTES + filter_bytes.len() + records.len(),
    );
    output.extend_from_slice(&header);
    for fence in fences {
        output.extend_from_slice(fence);
    }
    output.extend_from_slice(&filter_bytes);
    let structure_end = output.len();
    let structure_sha256 = index_structure_digest(
        output[..INDEX_HEADER_LEN]
            .try_into()
            .expect("index header length"),
        &output[INDEX_HEADER_LEN..structure_end],
    );
    output[INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END]
        .copy_from_slice(&structure_sha256);
    let tag = digest(&output[..INDEX_HEADER_TAG_START]);
    output[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN].copy_from_slice(&tag[..4]);
    output.extend_from_slice(&records);
    Ok((output, records_sha256))
}

/// Reads a v3 run header, fences, and filter into memory and performs the
/// integrity and structure checks before either accelerator can affect a
/// lookup. Records are never decoded here.
fn map_random_if_enabled(
    file: &File,
    len: u64,
    path: &Path,
    options: PackStoreOptions,
) -> Result<Option<Mmap>> {
    options
        .random_point_mmap
        .then(|| Mmap::map_random(file, len, path))
        .transpose()
}

#[cfg(test)]
fn read_index_run(path: &Path) -> Result<IndexRun> {
    read_index_run_with_options(path, PackStoreOptions::default())
}

fn read_index_run_with_options(path: &Path, options: PackStoreOptions) -> Result<IndexRun> {
    let file = File::open(path).with_context(|| format!("open index run {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("stat index run {}", path.display()))?
        .len();
    ensure!(
        file_len >= (INDEX_HEADER_LEN + INDEX_RECORD_LEN) as u64,
        "short index run {}",
        path.display()
    );
    let mut header = [0u8; INDEX_HEADER_LEN];
    file.read_exact_at(&mut header, 0)
        .with_context(|| format!("read index header {}", path.display()))?;
    ensure!(
        &header[0..8] == INDEX_MAGIC,
        "invalid index magic in {}",
        path.display()
    );
    ensure!(
        u32_at(&header, 8)? == PACK_INDEX_FORMAT_VERSION,
        "unsupported index version"
    );
    ensure!(
        u32_at(&header, 12)? as usize == INDEX_HEADER_LEN,
        "invalid index header length"
    );
    let tag = digest(&header[..INDEX_HEADER_TAG_START]);
    ensure!(
        header[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN] == tag[..4],
        "index header tag mismatch in {}",
        path.display()
    );
    ensure!(
        header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]
            .iter()
            .all(|byte| *byte == 0),
        "index header reserved bytes are non-zero in {}",
        path.display()
    );
    let epoch = u64_at(&header, 16)?;
    let record_count = u64_at(&header, 24)?;
    ensure!(record_count > 0, "empty index run {}", path.display());
    let records_sha256: [u8; 32] = header[32..64].try_into().expect("records checksum");
    let fence_count = usize::try_from(u32_at(&header, 64)?).context("fence count overflows")?;
    ensure!(
        u32_at(&header, 68)? as usize == FENCE_INTERVAL,
        "unsupported fence interval"
    );
    let records = usize::try_from(record_count).context("index count does not fit usize")?;
    ensure!(
        fence_count == records.div_ceil(FENCE_INTERVAL),
        "fence count mismatch in {}",
        path.display()
    );
    let seed = u64_at(&header, 72)?;
    let filter_count = usize::try_from(u32_at(&header, 80)?).context("filter size overflows")?;
    ensure!(
        u32_at(&header, 84)? == FILTER_FINGERPRINT_BITS,
        "unsupported filter fingerprint width"
    );
    let mut min_key = [0u8; PACK_KEY_BYTES];
    min_key.copy_from_slice(&header[88..121]);
    let mut max_key = [0u8; PACK_KEY_BYTES];
    max_key.copy_from_slice(&header[121..154]);
    ensure!(
        min_key <= max_key,
        "inverted index key range in {}",
        path.display()
    );
    let fence_bytes = fence_count * FENCE_KEY_BYTES;
    let filter_bytes = filter_count * 2;
    let records_offset = (INDEX_HEADER_LEN + fence_bytes + filter_bytes) as u64;
    let expected_len = records_offset
        .checked_add(
            record_count
                .checked_mul(INDEX_RECORD_LEN as u64)
                .context("index length overflows")?,
        )
        .context("index length overflows")?;
    ensure!(
        file_len == expected_len,
        "index run length mismatch in {}",
        path.display()
    );
    let mut structure = vec![0u8; fence_bytes + filter_bytes];
    file.read_exact_at(&mut structure, INDEX_HEADER_LEN as u64)
        .context("read index fences and filter")?;
    ensure!(
        index_structure_digest(&header, &structure).as_slice()
            == &header[INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END],
        "index structure checksum mismatch in {}",
        path.display()
    );
    let mut fences = Vec::with_capacity(fence_count);
    for chunk in structure[..fence_bytes].chunks_exact(FENCE_KEY_BYTES) {
        fences.push(chunk.try_into().expect("fence chunk"));
    }
    ensure!(
        fences.windows(2).all(|pair| pair[0] <= pair[1]),
        "index fences are not sorted"
    );
    let filter = XorFilter::decode(seed, &structure[fence_bytes..])
        .context("decode run membership filter")?;
    let mut first_record = [0u8; INDEX_RECORD_LEN];
    file.read_exact_at(&mut first_record, records_offset)
        .context("read first index record")?;
    let first = decode_record(&first_record)?;
    let mut last_record = [0u8; INDEX_RECORD_LEN];
    file.read_exact_at(
        &mut last_record,
        records_offset + (record_count - 1) * INDEX_RECORD_LEN as u64,
    )
    .context("read last index record")?;
    let last = decode_record(&last_record)?;
    ensure!(
        first.key == min_key && last.key == max_key,
        "index key range does not match its records in {}",
        path.display()
    );
    ensure!(
        fences.first() == Some(&truncate_key(&first.key)),
        "first fence does not match the first record in {}",
        path.display()
    );
    let map = Mmap::map(&file, file_len, path)?;
    let lookup_map = map_random_if_enabled(&file, file_len, path, options)?;
    drop(file);
    let memory_bytes = u64::try_from(fence_bytes)
        .context("structured bytes overflow")?
        .checked_add(filter.memory_bytes())
        .and_then(|total| total.checked_add(RUN_METADATA_BYTES))
        .context("structured bytes overflow")?;
    Ok(IndexRun {
        epoch,
        record_count,
        map,
        lookup_map,
        records_offset,
        file_bytes: file_len,
        min_key,
        max_key,
        min_prefix: key_prefix(&min_key),
        max_prefix: key_prefix(&max_key),
        fences,
        filter,
        records_sha256,
        memory_bytes,
    })
}

/// Fully re-verifies the records checksum of the newest run. This is the
/// only run payload read during open; older runs were verified when written
/// and are re-checked by scrubbing.
fn verify_tail_run(run: &IndexRun) -> Result<()> {
    let records_len = usize::try_from(
        run.record_count
            .checked_mul(INDEX_RECORD_LEN as u64)
            .context("tail run records length overflows")?,
    )
    .context("tail run records length does not fit usize")?;
    let records_start =
        usize::try_from(run.records_offset).context("records offset does not fit usize")?;
    let records = run
        .map
        .as_slice()
        .get(records_start..records_start + records_len)
        .context("read committed tail run records")?;
    ensure!(
        digest(records).as_slice() == run.records_sha256,
        "index records checksum mismatch in committed tail run"
    );
    Ok(())
}

/// Encodes and atomically publishes one fresh run file (tmp + sync + rename
/// + directory sync), then reads it back through the validating reader so
/// every published run is structurally verified before use.
fn publish_fresh_run(
    entries: &[IndexEntry],
    epoch: u64,
    runs_dir: &Path,
    file_name: &str,
    options: PackStoreOptions,
) -> Result<IndexRun> {
    ensure!(!entries.is_empty(), "cannot publish an empty index run");
    let min_key = entries.first().expect("non-empty run").key;
    let max_key = entries.last().expect("non-empty run").key;
    let fences = build_fences(entries);
    let keys = distinct_keys(entries);
    let filter =
        XorFilter::build(&keys, filter_seed(epoch)).context("build run membership filter")?;
    let (index_bytes, _) = encode_index_run(epoch, entries, &fences, &filter, &min_key, &max_key)?;
    let final_path = runs_dir.join(file_name);
    let temp_path = runs_dir.join(format!("{file_name}.tmp"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .with_context(|| format!("create index run {}", temp_path.display()))?;
    file.write_all(&index_bytes)
        .with_context(|| format!("write index run {}", temp_path.display()))?;
    file.sync_data()
        .with_context(|| format!("sync index run {}", temp_path.display()))?;
    drop(file);
    fs::rename(&temp_path, &final_path).with_context(|| {
        format!(
            "publish index run {} as {}",
            temp_path.display(),
            final_path.display()
        )
    })?;
    sync_directory(runs_dir)?;
    read_index_run_with_options(&final_path, options)
}

fn build_compacted_run_from_inputs(
    level: u32,
    inputs: &[LiveRun],
    runs_dir: &Path,
    random_point_mmap: bool,
) -> Result<PendingMerge> {
    ensure!(inputs.len() >= 2, "compaction requires at least two inputs");
    let min_epoch = inputs.first().expect("merge inputs").min_epoch;
    let max_epoch = inputs.last().expect("merge inputs").max_epoch;
    let mut merged = Vec::new();
    let mut input_records = 0u64;
    let mut input_memory_bytes = 0u64;
    for live in inputs {
        let record_bytes = usize::try_from(
            live.run
                .record_count
                .checked_mul(INDEX_RECORD_LEN as u64)
                .context("compaction input size overflows")?,
        )
        .context("compaction input size does not fit usize")?;
        let records_start =
            usize::try_from(live.run.records_offset).context("records offset overflows")?;
        let records = live
            .run
            .map
            .as_slice()
            .get(records_start..records_start + record_bytes)
            .context("compaction input records outside the run")?;
        // Compaction doubles as a scrub of every merged run.
        ensure!(
            digest(records).as_slice() == live.run.records_sha256,
            "compaction input records checksum mismatch"
        );
        for chunk in records.chunks_exact(INDEX_RECORD_LEN) {
            merged.push((live.max_epoch, decode_record(chunk)?));
        }
        input_records = input_records.saturating_add(live.run.record_count);
        input_memory_bytes = input_memory_bytes.saturating_add(live.run.memory_bytes);
    }
    // Newest epoch wins; within one epoch the newest frame row wins.
    merged.sort_unstable_by(|(left_epoch, left), (right_epoch, right)| {
        left.key
            .cmp(&right.key)
            .then_with(|| right_epoch.cmp(left_epoch))
            .then_with(|| right.sequence.cmp(&left.sequence))
    });
    let mut entries: Vec<IndexEntry> = Vec::with_capacity(merged.len());
    for (_, entry) in merged {
        if entries
            .last()
            .is_some_and(|last: &IndexEntry| last.key == entry.key)
        {
            continue;
        }
        entries.push(entry);
    }
    let output_records =
        u64::try_from(entries.len()).context("merged record count does not fit u64")?;
    let file_name = run_file_name(level + 1, min_epoch, max_epoch);
    let run = publish_fresh_run(
        &entries,
        max_epoch,
        runs_dir,
        &file_name,
        PackStoreOptions { random_point_mmap },
    )
    .context("publish compacted index run")?;
    Ok(PendingMerge {
        level: level + 1,
        min_epoch,
        max_epoch,
        run,
        input_runs: u64::try_from(inputs.len()).context("merge input count overflows")?,
        input_records,
        output_records,
        input_memory_bytes,
        inputs: Vec::new(),
        wall_ns: 0,
    })
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
        .open(&lease_path)
    {
        Ok(file) => (file, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lease_path)
                .with_context(|| format!("open writer lease {}", lease_path.display()))?,
            false,
        ),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("create writer lease {}", lease_path.display()));
        }
    };
    match lease.try_lock() {
        Ok(()) => {}
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
    if created {
        sync_directory(root)?;
    }
    Ok(lease)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn index_structure_digest(header: &[u8; INDEX_HEADER_LEN], structure: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(INDEX_STRUCTURE_DIGEST_DOMAIN);
    hasher.update(&header[..INDEX_STRUCTURE_SHA256_START]);
    hasher.update(&header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]);
    hasher.update(structure);
    hasher.finalize().into()
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
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom};
    use tempfile::tempdir;

    fn key(tag: u8) -> [u8; PACK_KEY_BYTES] {
        let mut key = [tag; PACK_KEY_BYTES];
        key[0] = TEST_NODE_PREFIX;
        key
    }

    const TEST_NODE_PREFIX: u8 = 0xf0;

    fn put(key: [u8; PACK_KEY_BYTES], value: &[u8]) -> PackOperation {
        PackOperation {
            key,
            kind: PackOpKind::Put(value.to_vec()),
        }
    }

    fn tombstone(key: [u8; PACK_KEY_BYTES]) -> PackOperation {
        PackOperation {
            key,
            kind: PackOpKind::Tombstone,
        }
    }

    fn small_compaction() -> CompactionConfig {
        CompactionConfig {
            l0_bound: 2,
            l1_bound: 2,
            fanout: 3,
        }
    }

    fn append_without_maintenance(store: &mut PackStore, operations: &[PackOperation]) {
        let prepared = store
            .prepare_append(operations)
            .expect("prepare unmaintained frame");
        let sealed = store
            .seal_prepared(prepared)
            .expect("seal unmaintained frame");
        drop(sealed.into_snapshot());
    }

    #[test]
    fn writer_lease_excludes_a_second_store_until_drop() {
        let root = tempdir().expect("temporary append store");
        let store = PackStore::create(root.path(), 1024 * 1024).expect("create first writer");

        let error = PackStore::open(root.path(), 1024 * 1024)
            .err()
            .expect("second writer must be rejected");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::WriterOwned { .. })
        ));

        drop(store);
        PackStore::open(root.path(), 1024 * 1024).expect("lease releases with writer drop");
    }

    #[cfg(unix)]
    #[test]
    fn writer_lease_canonicalizes_a_symlinked_store_path() {
        let parent = tempdir().expect("temporary pack parent");
        let root = parent.path().join("store");
        let alias = parent.path().join("store-alias");
        let store = PackStore::create(&root, 1024 * 1024).expect("create first writer");
        std::os::unix::fs::symlink(&root, &alias).expect("create store symlink");

        let error = PackStore::open(&alias, 1024 * 1024)
            .err()
            .expect("symlink alias must share the writer lease");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::WriterOwned { .. })
        ));

        drop(store);
        PackStore::open(&alias, 1024 * 1024).expect("alias opens after lease release");
    }

    #[test]
    fn frame_header_enforces_row_payload_and_allocation_hard_limits() {
        let checksum = [0u8; 32];
        let error = encode_frame_header(0, 0, 0, checksum)
            .expect_err("empty frame header must be rejected");
        assert!(error.to_string().contains("at least one row"));

        let too_many_rows = usize::try_from(MAX_FRAME_ROWS + 1).expect("row limit fits usize");
        let error = encode_frame_header(
            0,
            too_many_rows,
            usize::try_from(MAX_FRAME_PAYLOAD_BYTES).expect("payload limit fits usize"),
            checksum,
        )
        .expect_err("oversized row count must be rejected");
        assert!(error.to_string().contains("row count exceeds"));

        let error = encode_frame_header(
            0,
            1,
            usize::try_from(MAX_FRAME_PAYLOAD_BYTES + 1).expect("payload overflow fits usize"),
            checksum,
        )
        .expect_err("oversized payload must be rejected");
        assert!(error.to_string().contains("payload exceeds"));

        let short_payload = usize::try_from(2 * FRAME_ROW_HEADER_BYTES - 1)
            .expect("short payload length fits usize");
        let error = encode_frame_header(0, 2, short_payload, checksum)
            .expect_err("short row payload must be rejected");
        assert!(error.to_string().contains("too short"));

        let mut malicious = [0u8; FRAME_HEADER_LEN];
        malicious[0..8].copy_from_slice(FRAME_MAGIC);
        malicious[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
        malicious[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
        malicious[24..32].copy_from_slice(&1u64.to_le_bytes());
        malicious[32..40].copy_from_slice(&(MAX_FRAME_PAYLOAD_BYTES + 1).to_le_bytes());
        let error = validate_frame_header(&malicious, 0)
            .expect_err("oversized reopened payload must be rejected");
        assert!(error.to_string().contains("payload exceeds"));
    }

    #[test]
    fn random_point_mmaps_are_opt_in_and_survive_append_compaction_and_reopen() {
        let default_root = tempdir().expect("temporary default store");
        let mut default_store =
            PackStore::create(default_root.path(), 1024 * 1024).expect("create default store");
        default_store
            .append(&[put(key(1), b"default")])
            .expect("append through default mappings");
        assert!(default_store.lookup_pack_map.is_none());
        assert!(
            default_store
                .runs
                .iter()
                .all(|live| live.run.lookup_map.is_none())
        );

        let root = tempdir().expect("temporary random-mmap store");
        let options = PackStoreOptions {
            random_point_mmap: true,
        };
        let mut store = PackStore::create_with_compaction_and_options(
            root.path(),
            1024 * 1024,
            small_compaction(),
            options,
        )
        .expect("create random-mmap store");
        let first = key(10);
        let second = key(20);
        store
            .append(&[put(first, b"old"), put(second, b"present")])
            .expect("append initial versions");
        let pinned = store.snapshot().expect("pin initial generation");
        store
            .append(&[put(first, b"new")])
            .expect("append replacement");
        store
            .append(&[tombstone(second)])
            .expect("append tombstone and compact L0");

        assert!(store.lookup_pack_map.is_some());
        assert!(store.runs.iter().all(|live| live.run.lookup_map.is_some()));
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.runs[0].level, 1);
        assert_eq!(
            store.get(&first).expect("point replacement"),
            Some(b"new".to_vec())
        );
        assert_eq!(store.get(&second).expect("point tombstone"), None);
        assert_eq!(
            store
                .get_many_sorted(&[first, second])
                .expect("sorted current generation"),
            vec![Some(b"new".to_vec()), None]
        );
        assert_eq!(
            pinned.get(&first).expect("pinned point"),
            Some(b"old".to_vec())
        );
        assert_eq!(
            pinned
                .get_many_sorted(&[first, second])
                .expect("pinned sorted generation"),
            vec![Some(b"old".to_vec()), Some(b"present".to_vec())]
        );
        drop(pinned);
        drop(store);

        let reopened = PackStore::open_with_options(root.path(), 1024 * 1024, options)
            .expect("reopen with random mappings");
        assert!(reopened.lookup_pack_map.is_some());
        assert!(
            reopened
                .runs
                .iter()
                .all(|live| live.run.lookup_map.is_some())
        );
        assert_eq!(
            reopened.get(&first).expect("reopened point"),
            Some(b"new".to_vec())
        );
        assert_eq!(
            reopened
                .get_many_sorted(&[first, second])
                .expect("reopened sorted batch"),
            vec![Some(b"new".to_vec()), None]
        );
        let scrub = reopened
            .scrub_committed_frames()
            .expect("scrub normal mapping");
        assert_eq!(scrub.frames, 3);
        assert_eq!(scrub.tombstones, 1);
    }

    #[test]
    fn newest_row_and_run_win_and_tombstones_survive_reopen() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let first = key(1);
        let second = key(2);

        store
            .append(&[
                put(first, b"old"),
                put(second, b"second"),
                put(first, b"same-frame-new"),
            ])
            .expect("append first frame");
        assert_eq!(
            store.get(&first).expect("read same-frame version"),
            Some(b"same-frame-new".to_vec())
        );
        store
            .append(&[put(first, b"new-run")])
            .expect("append replacement frame");
        assert_eq!(
            store.get(&first).expect("read newer run"),
            Some(b"new-run".to_vec())
        );
        let sorted = store
            .get_many_sorted(&[first, second])
            .expect("read sorted keys");
        assert_eq!(
            sorted,
            vec![Some(b"new-run".to_vec()), Some(b"second".to_vec())]
        );
        store.append(&[tombstone(first)]).expect("append tombstone");
        assert_eq!(store.get(&first).expect("read tombstone"), None);
        drop(store);

        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen store");
        assert_eq!(reopened.get(&first).expect("read reopened tombstone"), None);
        assert_eq!(
            reopened.get(&second).expect("read reopened value"),
            Some(b"second".to_vec())
        );
        assert_eq!(reopened.open_validation().frames, 3);
        assert_eq!(reopened.open_validation().runs, 3);
    }

    #[test]
    fn sorted_batch_restores_key_order_after_payload_offset_reads() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let first = key(0x11);
        let second = key(0x22);
        let third = key(0x33);
        let missing = key(0x44);

        // Payload offsets follow operation order, deliberately opposing the
        // sorted query order used by the index scan.
        store
            .append(&[
                put(third, b"third"),
                put(second, b"second"),
                put(first, b"first"),
            ])
            .expect("append reverse-offset values");
        store
            .append(&[tombstone(second)])
            .expect("append second-key tombstone");

        assert_eq!(
            store
                .get_many_sorted(&[first, first, second, third, missing])
                .expect("read reordered batch"),
            vec![
                Some(b"first".to_vec()),
                Some(b"first".to_vec()),
                None,
                Some(b"third".to_vec()),
                None,
            ]
        );
        assert!(
            store
                .get_many_sorted(&[third, first])
                .expect_err("unsorted batch must fail")
                .to_string()
                .contains("sorted")
        );
    }

    #[test]
    fn append_rejects_decoded_index_memory_overflow_before_writing() {
        let root = tempdir().expect("temporary append store");
        let bound = std::mem::size_of::<IndexEntry>() as u64 - 1;
        let mut store = PackStore::create(root.path(), bound).expect("create bounded store");
        let error = store
            .append(&[put(key(3), b"value")])
            .expect_err("index memory bound must reject frame");
        assert!(error.to_string().contains("exceeds configured bound"));
        assert_eq!(store.layout().expect("empty layout"), (0, 0, 0, 0));
    }

    #[test]
    fn reopen_rejects_corrupt_committed_frame_payload() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(key(4), b"checksum-target")])
            .expect("append frame");
        drop(store);

        let pack_path = root.path().join("frames.pack");
        let mut pack = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pack_path)
            .expect("open pack for corruption");
        pack.seek(SeekFrom::Start(FRAME_HEADER_LEN as u64 + 1))
            .expect("seek into payload");
        pack.write_all(&[0xff]).expect("corrupt payload");
        pack.sync_all().expect("sync corruption");
        drop(pack);

        let error = PackStore::open(root.path(), 1024 * 1024)
            .err()
            .expect("corrupt frame must fail reopen");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    fn assert_corrupt_index_structure_is_rebuilt(byte_offset: u64) {
        let root = tempdir().expect("temporary append store");
        let target = key(5);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(target, b"structure-target")])
            .expect("append frame");
        let generation = store.snapshot().expect("snapshot").generation();
        drop(store);

        let run_path = root.path().join("runs").join(run_file_name(0, 0, 0));
        let mut run = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&run_path)
            .expect("open run for corruption");
        run.seek(SeekFrom::Start(byte_offset))
            .expect("seek to index structure byte");
        let mut byte = [0u8; 1];
        run.read_exact(&mut byte)
            .expect("read index structure byte");
        run.seek(SeekFrom::Start(byte_offset))
            .expect("rewind to index structure byte");
        run.write_all(&[byte[0] ^ 0x80])
            .expect("corrupt index structure byte");
        run.sync_all().expect("sync index structure corruption");
        drop(run);

        let error = read_index_run(&run_path).expect_err("structure corruption must be detected");
        assert!(error.to_string().contains("structure checksum mismatch"));

        let reopened = PackStore::open(root.path(), 1024 * 1024)
            .expect("corrupt derived run must rebuild from its committed frame");
        assert_eq!(
            reopened.get(&target).expect("read rebuilt value"),
            Some(b"structure-target".to_vec())
        );
        assert!(
            reopened.snapshot().expect("rebuilt snapshot").generation() > generation,
            "recovery must publish a new manifest generation"
        );
    }

    #[test]
    fn reopen_detects_and_rebuilds_corrupt_index_fence() {
        assert_corrupt_index_structure_is_rebuilt(INDEX_HEADER_LEN as u64);
    }

    #[test]
    fn reopen_detects_and_rebuilds_corrupt_index_filter() {
        assert_corrupt_index_structure_is_rebuilt((INDEX_HEADER_LEN + FENCE_KEY_BYTES) as u64);
    }

    #[test]
    fn legacy_v2_index_run_is_rebuilt_before_use() {
        let root = tempdir().expect("temporary append store");
        let target = key(6);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(target, b"legacy-index-target")])
            .expect("append frame");
        let generation = store.snapshot().expect("snapshot").generation();
        drop(store);

        let run_path = root.path().join("runs").join(run_file_name(0, 0, 0));
        let mut bytes = fs::read(&run_path).expect("read index run");
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        let tag = digest(&bytes[..INDEX_HEADER_TAG_START]);
        bytes[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN].copy_from_slice(&tag[..4]);
        fs::write(&run_path, bytes).expect("write legacy index version");

        let error = read_index_run(&run_path).expect_err("v2 run must require a rebuild");
        assert!(error.to_string().contains("unsupported index version"));

        let reopened = PackStore::open(root.path(), 1024 * 1024)
            .expect("legacy derived run must rebuild from its committed frame");
        assert_eq!(
            reopened.get(&target).expect("read rebuilt value"),
            Some(b"legacy-index-target".to_vec())
        );
        assert!(
            reopened.snapshot().expect("rebuilt snapshot").generation() > generation,
            "migration must publish a new manifest generation"
        );
        let rebuilt = fs::read(&run_path).expect("read rebuilt index run");
        assert_eq!(
            u32_at(&rebuilt, 8).expect("read rebuilt index version"),
            PACK_INDEX_FORMAT_VERSION
        );
    }

    #[test]
    fn committed_frame_scrub_counts_every_historical_row() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let first = key(41);
        let second = key(42);
        store
            .append(&[put(first, b"old"), put(second, b"second")])
            .expect("append first frame");
        store
            .append(&[put(first, b"new"), tombstone(second)])
            .expect("append second frame");
        let generation_before = store
            .snapshot()
            .expect("snapshot before republish")
            .generation();
        store
            .republish_manifest()
            .expect("republish unchanged manifest");
        let generation_after = store
            .snapshot()
            .expect("snapshot after republish")
            .generation();
        assert_eq!(generation_after, generation_before + 1);

        let scrub = store.scrub_committed_frames().expect("scrub frames");
        assert_eq!(scrub.frames, 2);
        assert_eq!(scrub.rows, 4);
        assert_eq!(scrub.puts, 3);
        assert_eq!(scrub.tombstones, 1);
        assert_eq!(scrub.value_bytes, 3 + 6 + 3);
        assert!(scrub.payload_bytes > scrub.value_bytes);
    }

    #[test]
    fn checkpoint_scrub_hashes_every_ordered_key_and_value() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let first = key(45);
        let second = key(46);
        store
            .append(&[put(first, b"first")])
            .expect("append first checkpoint frame");
        store
            .append(&[put(second, b"second")])
            .expect("append second checkpoint frame");

        let evidence = store
            .scrub_checkpoint_namespace()
            .expect("scrub checkpoint namespace");
        let mut expected = Sha256::new();
        expected.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        for (key, value) in [(first, b"first".as_slice()), (second, b"second".as_slice())] {
            expected.update((PACK_KEY_BYTES as u32).to_le_bytes());
            expected.update(key);
            expected.update((value.len() as u64).to_le_bytes());
            expected.update(value);
        }
        assert_eq!(evidence.sha256, <[u8; 32]>::from(expected.finalize()));
        assert_eq!(evidence.scrub.frames, 2);
        assert_eq!(evidence.scrub.rows, 2);
        assert_eq!(evidence.scrub.puts, 2);
        assert_eq!(evidence.scrub.tombstones, 0);
        assert_eq!(evidence.scrub.value_bytes, 11);
    }

    #[test]
    fn checkpoint_scrub_rejects_versioned_or_tombstoned_streams() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let repeated = key(47);
        store
            .append(&[put(repeated, b"first")])
            .expect("append first version");
        store
            .append(&[put(repeated, b"second")])
            .expect("append repeated version");
        let error = store
            .scrub_checkpoint_namespace()
            .expect_err("checkpoint scrub must reject repeated keys");
        assert!(error.to_string().contains("not strictly increasing"));

        let tombstone_root = tempdir().expect("temporary tombstone store");
        let mut tombstone_store =
            PackStore::create(tombstone_root.path(), 1024 * 1024).expect("create store");
        tombstone_store
            .append(&[tombstone(key(48))])
            .expect("append tombstone");
        let error = tombstone_store
            .scrub_checkpoint_namespace()
            .expect_err("checkpoint scrub must reject tombstones");
        assert!(error.to_string().contains("contains a tombstone"));
    }

    #[test]
    fn committed_frame_scrub_detects_corrupt_non_tail_payload() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(key(43), b"first")])
            .expect("append first frame");
        store
            .append(&[put(key(44), b"tail")])
            .expect("append tail frame");
        drop(store);

        let pack_path = root.path().join("frames.pack");
        let mut pack = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pack_path)
            .expect("open pack for corruption");
        pack.seek(SeekFrom::Start(
            FRAME_HEADER_LEN as u64 + FRAME_ROW_HEADER_BYTES,
        ))
        .expect("seek into first value");
        pack.write_all(&[0xff]).expect("corrupt first payload");
        pack.sync_all().expect("sync corruption");
        drop(pack);

        let reopened = PackStore::open(root.path(), 1024 * 1024)
            .expect("normal open verifies only the committed tail payload");
        let error = reopened
            .scrub_committed_frames()
            .expect_err("full scrub must reject the older corrupt frame");
        assert!(
            error
                .to_string()
                .contains("frame 0 payload checksum mismatch")
        );
    }

    #[test]
    fn fence_probe_handles_boundaries_and_truncated_prefix_collisions() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        // 200 keys sharing the first 25 bytes: every fence prefix collides,
        // so probes widen to the whole run and spill off the stack buffer.
        // Even ordinals leave in-range gaps for absent-key probes.
        let mut keys = Vec::new();
        for ordinal in (0u64..400).step_by(2) {
            let mut key = [0xAAu8; PACK_KEY_BYTES];
            key[0] = TEST_NODE_PREFIX;
            key[25..33].copy_from_slice(&ordinal.to_be_bytes());
            keys.push(key);
        }
        let operations: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| put(*key, format!("value-{index}").as_bytes()))
            .collect();
        store.append(&operations).expect("append adversarial frame");
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(
                store.get(key).expect("read boundary key"),
                Some(format!("value-{index}").into_bytes())
            );
        }
        for ordinal in (1u64..400).step_by(2) {
            let mut absent = [0xAAu8; PACK_KEY_BYTES];
            absent[0] = TEST_NODE_PREFIX;
            absent[25..33].copy_from_slice(&ordinal.to_be_bytes());
            assert_eq!(store.get(&absent).expect("read absent in-range key"), None);
        }
        let mut below = [0xAAu8; PACK_KEY_BYTES];
        below[0] = 0x10;
        assert_eq!(store.get(&below).expect("read below-range key"), None);
        let mut above = [0xAAu8; PACK_KEY_BYTES];
        above[0] = TEST_NODE_PREFIX;
        above[25..33].copy_from_slice(&10_000u64.to_be_bytes());
        assert_eq!(store.get(&above).expect("read above-range key"), None);
        let sorted = store
            .get_many_sorted(&keys)
            .expect("batch read boundary keys");
        for (index, value) in sorted.iter().enumerate() {
            assert_eq!(value.as_deref(), Some(format!("value-{index}").as_bytes()));
        }
        drop(store);

        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen store");
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(
                reopened.get(key).expect("read reopened boundary key"),
                Some(format!("value-{index}").into_bytes())
            );
        }
    }

    #[test]
    fn reopen_truncates_torn_tail_without_a_published_run() {
        let root = tempdir().expect("temporary append store");
        let first = key(1);
        let second = key(2);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(first, b"one")])
            .expect("append first frame");
        store
            .append(&[put(second, b"two")])
            .expect("append second frame");
        let (committed_len, _, _, _) = store.layout().expect("committed layout");
        drop(store);

        // Case one: torn partial frame header at the tail.
        let pack_path = root.path().join("frames.pack");
        let mut pack = OpenOptions::new()
            .append(true)
            .open(&pack_path)
            .expect("open pack for torn header");
        pack.write_all(&[0xABu8; 50]).expect("write torn header");
        pack.sync_all().expect("sync torn header");
        drop(pack);

        let reopened = PackStore::open(root.path(), 1024 * 1024)
            .expect("reopen truncates a torn partial header");
        assert_eq!(reopened.open_validation().frames, 2);
        assert_eq!(
            reopened.get(&first).expect("read first after truncation"),
            Some(b"one".to_vec())
        );
        assert_eq!(
            reopened.get(&second).expect("read second after truncation"),
            Some(b"two".to_vec())
        );
        assert_eq!(
            reopened.layout().expect("truncated layout").0,
            committed_len
        );
        drop(reopened);

        // Case two: well-formed header whose payload is torn.
        let mut pack = OpenOptions::new()
            .append(true)
            .open(&pack_path)
            .expect("open pack for torn payload");
        let mut fake = [0u8; FRAME_HEADER_LEN];
        fake[0..8].copy_from_slice(FRAME_MAGIC);
        fake[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
        fake[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
        fake[16..24].copy_from_slice(&2u64.to_le_bytes());
        fake[24..32].copy_from_slice(&1u64.to_le_bytes());
        fake[32..40].copy_from_slice(&1_000_000u64.to_le_bytes());
        pack.write_all(&fake).expect("write torn frame header");
        pack.write_all(&[0xCDu8; 128])
            .expect("write torn payload bytes");
        pack.sync_all().expect("sync torn payload");
        drop(pack);

        let reopened =
            PackStore::open(root.path(), 1024 * 1024).expect("reopen truncates a torn payload");
        assert_eq!(reopened.open_validation().frames, 2);
        assert_eq!(
            reopened
                .get(&second)
                .expect("read second after payload truncation"),
            Some(b"two".to_vec())
        );
        assert_eq!(
            reopened.layout().expect("truncated layout").0,
            committed_len
        );
    }

    #[test]
    fn compaction_dedups_and_pinned_generation_reads_older_versions() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create_with_compaction(root.path(), 1024 * 1024, small_compaction())
                .expect("create store");
        let target = key(1);
        store
            .append(&[put(target, b"v1"), put(key(2), b"a")])
            .expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        let pinned = store.snapshot().expect("pin generation 2");
        assert_eq!(pinned.generation(), 2);

        // Frame 2 pushes L0 past its bound: the first compaction cycle merges
        // all three frames into one L1 run, keeping the newest version.
        store.append(&[put(target, b"v3")]).expect("append frame 2");
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.runs[0].level, 1);
        assert_eq!(
            store.get(&target).expect("read compacted newest"),
            Some(b"v3".to_vec())
        );
        assert_eq!(
            pinned.get(&target).expect("read pinned older"),
            Some(b"v2".to_vec())
        );

        // Drive L1 into L2: three more L0 cycles, then one L1 merge.
        store.append(&[put(key(3), b"b")]).expect("append frame 3");
        store.append(&[put(key(4), b"c")]).expect("append frame 4");
        store.append(&[put(target, b"v4")]).expect("append frame 5");
        store.append(&[put(key(5), b"d")]).expect("append frame 6");
        store.append(&[put(key(6), b"e")]).expect("append frame 7");
        store.append(&[put(key(7), b"f")]).expect("append frame 8");
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.runs[0].level, 2);
        assert_eq!(store.runs[0].min_epoch, 0);
        assert_eq!(store.runs[0].max_epoch, 8);
        assert_eq!(
            store.get(&target).expect("read L2 newest"),
            Some(b"v4".to_vec())
        );
        assert_eq!(
            pinned.get(&target).expect("pinned still older"),
            Some(b"v2".to_vec())
        );

        let stats = store.compaction_stats();
        assert_eq!(stats.cycles, 4);
        assert_eq!(stats.runs_merged, 12);
        assert_eq!(stats.runs_produced, 4);
        assert!(
            stats.output_records < stats.input_records,
            "dedup must drop superseded versions: {stats:?}"
        );
        drop(pinned);
        drop(store);

        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen compacted store");
        assert_eq!(
            reopened.get(&target).expect("read reopened L2"),
            Some(b"v4".to_vec())
        );
        assert_eq!(reopened.open_validation().runs, 1);
        assert_eq!(reopened.open_validation().frames, 9);
    }

    #[test]
    fn compaction_plan_builds_without_the_writer_and_preserves_later_appends() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create_with_compaction(root.path(), 1024 * 1024, small_compaction())
                .expect("create store");
        let target = key(80);
        append_without_maintenance(&mut store, &[put(target, b"v0")]);
        append_without_maintenance(&mut store, &[put(target, b"v1")]);
        append_without_maintenance(&mut store, &[put(target, b"v2")]);
        let debt = store.compaction_debt();
        assert_eq!(debt.excess_runs, 1);
        assert!(!debt.backpressure_required);

        let plan = store
            .plan_compaction()
            .expect("plan compaction")
            .expect("overfull L0 has a plan");
        // The immutable plan no longer borrows the writer. A canonical append
        // can therefore land while the derived output is being built.
        append_without_maintenance(&mut store, &[put(target, b"v3")]);
        store.gc().expect("gc honors the plan's source lease");
        let prepared = plan.build().expect("build compacted output");
        store
            .adopt_compaction(prepared)
            .expect("adopt against the later generation");

        assert_eq!(
            store.get(&target).expect("read latest after adoption"),
            Some(b"v3".to_vec())
        );
        assert!(store.runs.iter().any(|live| live.level == 0));
        assert!(store.runs.iter().any(|live| live.level == 1));
        drop(store);
        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen compacted store");
        assert_eq!(
            reopened.get(&target).expect("read latest after reopen"),
            Some(b"v3".to_vec())
        );
    }

    #[test]
    fn leveled_compaction_bounds_levels_beyond_l2() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create_with_compaction(root.path(), 1024 * 1024, small_compaction())
                .expect("create store");
        for frame in 0..27u8 {
            store
                .append(&[put(key(frame), &[frame])])
                .expect("append recursively compacted frame");
        }
        let debt = store.compaction_debt();
        assert_eq!(debt.excess_runs, 0, "all levels stay within bounds");
        assert!(!debt.backpressure_required);
        assert!(
            store.runs.iter().any(|live| live.level >= 3),
            "long-running stores must compact beyond the former L2 ceiling"
        );
        for level in 0..=store.runs.iter().map(|live| live.level).max().unwrap_or(0) {
            assert!(
                store.runs.iter().filter(|live| live.level == level).count() <= 2,
                "level {level} exceeded its configured run bound"
            );
        }
    }

    #[test]
    fn lease_prevents_reclamation_until_snapshot_release() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store =
            PackStore::create_with_compaction(root.path(), 1024 * 1024, small_compaction())
                .expect("create store");
        let target = key(1);
        store.append(&[put(target, b"v1")]).expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        let pinned = store.snapshot().expect("pin generation 2");
        store
            .append(&[put(target, b"v3")])
            .expect("append frame 2 compacts");
        assert_eq!(store.runs.len(), 1);

        let first = store.gc().expect("gc with pinned lease");
        // run-2 is listed only by the superseded pre-compaction generation
        // and is reclaimed; the leased generation's runs must survive.
        assert_eq!(first.runs_deleted, 1, "only unprotected runs go");
        assert_eq!(first.manifests_deleted, 2, "only unprotected manifests go");
        for epoch in 0..2 {
            assert!(
                runs_dir.join(run_file_name(0, epoch, epoch)).exists(),
                "leased run {epoch} must be kept"
            );
        }
        assert!(!runs_dir.join(run_file_name(0, 2, 2)).exists());
        assert!(root.path().join(manifest::manifest_file_name(2)).exists());
        assert!(!root.path().join(manifest::manifest_file_name(1)).exists());
        assert_eq!(
            pinned.get(&target).expect("read through gc"),
            Some(b"v2".to_vec())
        );

        drop(pinned);
        let second = store.gc().expect("gc after lease release");
        assert_eq!(
            second.runs_deleted, 2,
            "leased runs reclaimed after release"
        );
        assert_eq!(second.manifests_deleted, 1, "released manifest reclaimed");
        for epoch in 0..3 {
            assert!(!runs_dir.join(run_file_name(0, epoch, epoch)).exists());
        }
        assert!(
            runs_dir
                .join("run-l1-00000000000000000000-00000000000000000002.idx")
                .exists(),
            "live compacted run stays"
        );
        assert_eq!(
            store.get(&target).expect("read after reclamation"),
            Some(b"v3".to_vec())
        );
        let stats = store.compaction_stats();
        assert_eq!(stats.gc_cycles, 2);
        assert_eq!(stats.gc_runs_deleted, 3);
    }

    #[test]
    fn crash_mid_compaction_keeps_previous_generation_live() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let target = key(1);
        store
            .append(&[put(target, b"v1"), put(key(2), b"a")])
            .expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        store.append(&[put(target, b"v3")]).expect("append frame 2");

        // Publish the merge output run file but drop the store before the
        // manifest publication: exactly a crash between the two atomic steps.
        let pending = store
            .build_compacted_run(0)
            .expect("merge oldest runs")
            .expect("three runs are mergeable");
        let orphan = runs_dir.join(run_file_name(
            pending.level,
            pending.min_epoch,
            pending.max_epoch,
        ));
        assert!(orphan.exists());
        drop(store);

        let mut reopened =
            PackStore::open(root.path(), 1024 * 1024).expect("reopen after interrupted compaction");
        assert_eq!(
            reopened.open_validation().runs,
            3,
            "orphan run is invisible"
        );
        assert_eq!(
            reopened.get(&target).expect("read after crash"),
            Some(b"v3".to_vec())
        );
        assert_eq!(
            reopened.get(&key(2)).expect("read sibling after crash"),
            Some(b"a".to_vec())
        );
        assert!(orphan.exists(), "gc did not run yet");
        let stats = reopened
            .gc()
            .expect("reclaim interrupted compaction output");
        assert_eq!(stats.runs_deleted, 1);
        assert!(!orphan.exists());
        assert_eq!(
            reopened.get(&target).expect("read after reclamation"),
            Some(b"v3".to_vec())
        );

        // A crashed append leaves a stale temp file; open clears it so the
        // next publication does not trip over create-new.
        let stale = runs_dir.join("run-00000000000000000003.tmp");
        fs::write(&stale, b"torn").expect("plant stale temp file");
        drop(reopened);
        let mut cleared = PackStore::open(root.path(), 1024 * 1024).expect("reopen clears stale");
        assert!(!stale.exists());
        cleared
            .append(&[put(target, b"v4")])
            .expect("append after stale temp cleanup");
        assert_eq!(
            cleared.get(&target).expect("read appended value"),
            Some(b"v4".to_vec())
        );
    }

    #[test]
    fn reopen_after_compaction_matches_precompaction_byte_for_byte() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create_with_compaction(root.path(), 1024 * 1024, small_compaction())
                .expect("create store");
        let mut model: Vec<(bool, [u8; PACK_KEY_BYTES], Option<Vec<u8>>)> = Vec::new();
        for tag in 0..16u8 {
            model.push((false, key(tag), None));
        }
        // Twelve frames drive L0 merges, an L1 merge into L2, tombstones,
        // and rewrites of earlier keys at every level.
        for frame in 0..12u8 {
            let mut operations = Vec::new();
            for ordinal in 0..8u8 {
                let tag = frame.wrapping_mul(3).wrapping_add(ordinal) % 16;
                let value = format!("f{frame}-v{ordinal}");
                operations.push(put(key(tag), value.as_bytes()));
                model[usize::from(tag)] = (true, key(tag), Some(value.into_bytes()));
            }
            if frame % 4 == 3 {
                let tag = (frame + 5) % 16;
                operations.push(tombstone(key(tag)));
                model[usize::from(tag)] = (true, key(tag), None);
            }
            store.append(&operations).expect("append model frame");
            assert_full_scan_matches(&store, &model);
        }
        let all_keys: Vec<_> = model.iter().map(|(_, key, _)| *key).collect();
        let before = store
            .get_many_sorted(&all_keys)
            .expect("capture pre-reopen reads");
        assert!(
            store.runs.iter().any(|live| live.level == 2),
            "L2 must be exercised"
        );
        let stats = store.compaction_stats();
        assert!(
            stats.cycles >= 4,
            "several compaction cycles ran: {stats:?}"
        );
        drop(store);

        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen compacted store");
        let after = reopened
            .get_many_sorted(&all_keys)
            .expect("read reopened compacted store");
        assert_eq!(before, after, "reopen reads diverged after compaction");
        assert_full_scan_matches(&reopened, &model);
    }

    fn assert_full_scan_matches(
        store: &PackStore,
        model: &[(bool, [u8; PACK_KEY_BYTES], Option<Vec<u8>>)],
    ) {
        let touched: Vec<_> = model.iter().filter(|entry| entry.0).collect();
        let keys: Vec<_> = touched.iter().map(|(_, key, _)| *key).collect();
        let actual = store.get_many_sorted(&keys).expect("full sorted scan");
        let expected: Vec<_> = touched.iter().map(|(_, _, value)| value.clone()).collect();
        assert_eq!(actual, expected, "store diverged from the model");
    }

    #[test]
    fn external_horizon_rebuilds_missing_manifest_and_runs_from_frames() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let target = key(1);
        store
            .append(&[put(target, b"v1"), put(key(2), b"a")])
            .expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        store.append(&[put(key(3), b"c")]).expect("append frame 2");
        let committed = store.last_frame_receipt().expect("committed receipt");
        let horizon = PackCommitHorizon {
            epoch: committed.epoch,
            payload_sha256: committed.payload_sha256,
        };
        drop(store);

        // A missing derived manifest is recoverable only with the explicit
        // canonical horizon. Raw frames alone are not a commit decision.
        for (_, path) in manifest::list_manifest_files(root.path()).expect("list manifests") {
            fs::remove_file(path).expect("delete manifest");
        }
        let reopened = PackStore::open_at_commit_horizon(root.path(), 1024 * 1024, Some(horizon))
            .expect("marker rebuilds the derived generation");
        assert_eq!(reopened.open_validation().frames, 3);
        assert_eq!(reopened.open_validation().runs, 3);
        assert_eq!(
            reopened.get(&target).expect("read reconstructed"),
            Some(b"v2".to_vec())
        );
        let republished = manifest::list_manifest_files(root.path()).expect("list republished");
        assert_eq!(
            republished.len(),
            1,
            "marker recovery republishes one generation"
        );
        assert_eq!(
            republished[0].0, 1,
            "generation restarts after a total manifest loss"
        );
        drop(reopened);

        // Lose the manifest again plus one run: the same marker deterministically
        // rebuilds every run from the committed frame prefix.
        for (_, path) in manifest::list_manifest_files(root.path()).expect("list manifests") {
            fs::remove_file(path).expect("delete manifest");
        }
        fs::remove_file(runs_dir.join(run_file_name(0, 1, 1))).expect("delete one run");
        let mut rebuilt =
            PackStore::open_at_commit_horizon(root.path(), 1024 * 1024, Some(horizon))
                .expect("marker rebuilds missing runs from frames");
        assert_eq!(rebuilt.open_validation().frames, 3);
        assert_eq!(rebuilt.open_validation().runs, 3);
        assert_eq!(
            rebuilt.get(&target).expect("read rebuilt"),
            Some(b"v2".to_vec())
        );
        assert_eq!(
            rebuilt.get(&key(3)).expect("read rebuilt sibling"),
            Some(b"c".to_vec())
        );
        // The store keeps appending at the right epoch after a rebuild.
        rebuilt
            .append(&[put(target, b"v3")])
            .expect("append after rebuild");
        assert_eq!(
            rebuilt.get(&target).expect("read post-rebuild append"),
            Some(b"v3".to_vec())
        );
    }

    #[test]
    fn prepared_append_is_invisible_in_process_and_without_an_external_horizon() {
        let empty_root = tempdir().expect("temporary empty store");
        let prepared_key = key(7);
        let mut empty = PackStore::create(empty_root.path(), 1024 * 1024).expect("create store");
        let prepared = empty
            .prepare_append(&[put(prepared_key, b"prepared-only")])
            .expect("prepare first frame");
        assert_eq!(prepared.receipt().epoch, 0);
        assert_eq!(prepared.stage_totals().frames, 1);
        assert_eq!(empty.get(&prepared_key).expect("read prepared key"), None);
        assert_eq!(empty.last_frame_receipt(), None);
        assert_eq!(empty.open_validation().frames, 0);
        drop(empty);

        let reopened = PackStore::open(empty_root.path(), 1024 * 1024)
            .expect("plain reopen discards an unactivated first frame");
        assert_eq!(reopened.open_validation().frames, 0);
        assert_eq!(
            reopened.get(&prepared_key).expect("read after reopen"),
            None
        );
        assert_eq!(reopened.layout().expect("recovered empty layout").0, 0);

        let prefix_root = tempdir().expect("temporary prefixed store");
        let committed_key = key(1);
        let orphan_key = key(2);
        let mut prefixed =
            PackStore::create(prefix_root.path(), 1024 * 1024).expect("create prefixed store");
        prefixed
            .append(&[put(committed_key, b"committed")])
            .expect("append committed prefix");
        let committed = prefixed.last_frame_receipt().expect("committed receipt");
        prefixed
            .prepare_append(&[
                put(committed_key, b"unactivated-replacement"),
                put(orphan_key, b"orphan"),
            ])
            .expect("prepare suffix");
        assert_eq!(
            prefixed.get(&committed_key).expect("read visible prefix"),
            Some(b"committed".to_vec())
        );
        assert_eq!(prefixed.get(&orphan_key).expect("read orphan key"), None);
        assert_eq!(prefixed.last_frame_receipt(), Some(committed));
        drop(prefixed);

        let reopened = PackStore::open(prefix_root.path(), 1024 * 1024)
            .expect("plain reopen keeps only manifested prefix");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(
            reopened.get(&committed_key).expect("read committed key"),
            Some(b"committed".to_vec())
        );
        assert_eq!(reopened.get(&orphan_key).expect("read discarded key"), None);
    }

    #[test]
    fn sealed_append_pins_the_new_generation_while_old_snapshots_stay_old() {
        let root = tempdir().expect("temporary sealed store");
        let target = key(4);
        let added = key(5);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(target, b"old")])
            .expect("append committed prefix");
        let old_snapshot = store.snapshot().expect("pin old snapshot");

        let prepared = store
            .prepare_append(&[put(target, b"new"), put(added, b"added")])
            .expect("prepare next generation");
        let expected_horizon = prepared.commit_horizon();
        let sealed = store
            .seal_prepared(prepared)
            .expect("seal prepared generation");

        assert_eq!(sealed.commit_horizon(), expected_horizon);
        assert_eq!(
            old_snapshot.get(&target).expect("read old target"),
            Some(b"old".to_vec())
        );
        assert_eq!(old_snapshot.get(&added).expect("read old added key"), None);
        assert_eq!(
            sealed.snapshot().get(&target).expect("read sealed target"),
            Some(b"new".to_vec())
        );
        assert_eq!(
            sealed
                .snapshot()
                .get(&added)
                .expect("read sealed added key"),
            Some(b"added".to_vec())
        );
        assert!(sealed.snapshot().generation() > old_snapshot.generation());

        let activated_snapshot = sealed.into_snapshot();
        assert_eq!(
            activated_snapshot
                .get(&target)
                .expect("read consumed sealed snapshot"),
            Some(b"new".to_vec())
        );
    }

    #[test]
    fn prior_horizon_discards_a_sealed_but_uncommitted_suffix() {
        let root = tempdir().expect("temporary sealed recovery store");
        let target = key(6);
        let suffix_only = key(7);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(target, b"committed")])
            .expect("append committed prefix");
        let committed = store.last_frame_receipt().expect("committed receipt");
        let prior_horizon = PackCommitHorizon {
            epoch: committed.epoch,
            payload_sha256: committed.payload_sha256,
        };

        let prepared = store
            .prepare_append(&[
                put(target, b"sealed-uncommitted"),
                put(suffix_only, b"suffix-only"),
            ])
            .expect("prepare suffix");
        let sealed = store
            .seal_prepared(prepared)
            .expect("seal provisional suffix");
        assert_eq!(
            sealed.snapshot().get(&target).expect("read sealed value"),
            Some(b"sealed-uncommitted".to_vec())
        );
        drop(sealed);
        drop(store);

        let reopened =
            PackStore::open_at_commit_horizon(root.path(), 1024 * 1024, Some(prior_horizon))
                .expect("reopen at preceding canonical horizon");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(reopened.last_frame_receipt(), Some(committed));
        assert_eq!(
            reopened.get(&target).expect("read committed target"),
            Some(b"committed".to_vec())
        );
        assert_eq!(
            reopened.get(&suffix_only).expect("read discarded suffix"),
            None
        );
    }

    #[test]
    fn activation_publishes_the_prepared_view_and_survives_reopen() {
        let root = tempdir().expect("temporary append store");
        let target = key(5);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let prepared = store
            .prepare_append(&[put(target, b"activated")])
            .expect("prepare frame");
        assert_eq!(store.get(&target).expect("read before activation"), None);
        store
            .activate_prepared(prepared, prepared.commit_horizon())
            .expect("activate prepared frame");
        assert_eq!(store.last_frame_receipt(), Some(prepared.receipt()));
        assert_eq!(
            store.get(&target).expect("read activated value"),
            Some(b"activated".to_vec())
        );
        drop(store);

        let reopened = PackStore::open(root.path(), 1024 * 1024).expect("reopen activated store");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(
            reopened.get(&target).expect("read reopened value"),
            Some(b"activated".to_vec())
        );
    }

    #[test]
    fn committed_marker_recovers_a_crash_before_in_process_activation() {
        let root = tempdir().expect("temporary append store");
        let target = key(6);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let prepared = store
            .prepare_append(&[put(target, b"marker-committed")])
            .expect("prepare frame");
        let horizon = prepared.commit_horizon();
        drop(store);

        let reopened = PackStore::open_at_commit_horizon(root.path(), 1024 * 1024, Some(horizon))
            .expect("marker rebuilds missing activation index");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(reopened.last_frame_receipt(), Some(prepared.receipt()));
        assert_eq!(
            reopened.get(&target).expect("read marker-recovered value"),
            Some(b"marker-committed".to_vec())
        );
    }

    #[test]
    fn activation_rejects_errors_duplicates_and_reordering_without_visibility() {
        let root = tempdir().expect("temporary append store");
        let first_key = key(10);
        let second_key = key(11);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        let first = store
            .prepare_append(&[put(first_key, b"first")])
            .expect("prepare first frame");
        assert!(
            store
                .prepare_append(&[put(second_key, b"blocked")])
                .is_err(),
            "a second prepare must not pass the pending frame"
        );
        assert!(store.gc().is_err(), "gc must not reclaim a pending run");

        let mut wrong_checksum = first.commit_horizon();
        wrong_checksum.payload_sha256[0] ^= 0x80;
        let checksum_error = store
            .activate_prepared(first, wrong_checksum)
            .expect_err("wrong marker checksum must fail");
        assert!(checksum_error.to_string().contains("checksum"));
        assert_eq!(store.get(&first_key).expect("read after bad marker"), None);

        let wrong_epoch = PackCommitHorizon {
            epoch: first.receipt().epoch + 1,
            payload_sha256: first.receipt().payload_sha256,
        };
        let epoch_error = store
            .activate_prepared(first, wrong_epoch)
            .expect_err("wrong marker epoch must fail");
        assert!(epoch_error.to_string().contains("epoch"));
        assert_eq!(store.get(&first_key).expect("read after bad epoch"), None);

        let forged = PreparedAppend {
            serial: first.serial + 1,
            ..first
        };
        let token_error = store
            .activate_prepared(forged, first.commit_horizon())
            .expect_err("forged token must fail");
        assert!(token_error.to_string().contains("token"));
        assert_eq!(store.get(&first_key).expect("read after bad token"), None);

        store
            .activate_prepared(first, first.commit_horizon())
            .expect("activate first frame");
        let duplicate_error = store
            .activate_prepared(first, first.commit_horizon())
            .expect_err("duplicate activation must fail");
        assert!(duplicate_error.to_string().contains("no prepared append"));
        assert_eq!(
            store.get(&first_key).expect("first remains visible"),
            Some(b"first".to_vec())
        );

        let second = store
            .prepare_append(&[put(second_key, b"second")])
            .expect("prepare second frame");
        let stale_error = store
            .activate_prepared(first, second.commit_horizon())
            .expect_err("stale token must not activate a later frame");
        assert!(stale_error.to_string().contains("token"));
        assert_eq!(store.get(&second_key).expect("read reordered frame"), None);
        store
            .activate_prepared(second, second.commit_horizon())
            .expect("activate second frame in order");
        assert_eq!(
            store.get(&second_key).expect("read second frame"),
            Some(b"second".to_vec())
        );
    }

    #[test]
    fn activation_revalidates_prepared_frame_and_run_before_publication() {
        let frame_root = tempdir().expect("temporary frame-corruption store");
        let frame_key = key(12);
        let mut frame_store =
            PackStore::create(frame_root.path(), 1024 * 1024).expect("create frame store");
        let frame_prepared = frame_store
            .prepare_append(&[put(frame_key, b"frame-target")])
            .expect("prepare frame target");
        let mut pack = OpenOptions::new()
            .read(true)
            .write(true)
            .open(frame_root.path().join("frames.pack"))
            .expect("open prepared pack");
        pack.seek(SeekFrom::Start(FRAME_HEADER_LEN as u64 + 1))
            .expect("seek into prepared payload");
        pack.write_all(&[0x7f]).expect("corrupt prepared payload");
        pack.sync_all().expect("sync prepared payload corruption");
        drop(pack);
        let frame_error = frame_store
            .activate_prepared(frame_prepared, frame_prepared.commit_horizon())
            .expect_err("corrupt prepared frame must not activate");
        assert!(frame_error.to_string().contains("checksum mismatch"));
        assert_eq!(
            frame_store.get(&frame_key).expect("read corrupt frame"),
            None
        );
        assert!(
            manifest::list_manifest_files(frame_root.path())
                .expect("list frame manifests")
                .is_empty()
        );

        let run_root = tempdir().expect("temporary run-corruption store");
        let run_key = key(13);
        let mut run_store =
            PackStore::create(run_root.path(), 1024 * 1024).expect("create run store");
        let run_prepared = run_store
            .prepare_append(&[put(run_key, b"run-target")])
            .expect("prepare run target");
        let run_path = run_root.path().join("runs").join(run_file_name(0, 0, 0));
        let mut run = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&run_path)
            .expect("open prepared run");
        run.seek(SeekFrom::End(-1)).expect("seek to run tail");
        let mut byte = [0u8; 1];
        run.read_exact(&mut byte).expect("read run tail");
        run.seek(SeekFrom::End(-1)).expect("rewind to run tail");
        run.write_all(&[byte[0] ^ 0x80])
            .expect("corrupt prepared run");
        run.sync_all().expect("sync prepared run corruption");
        drop(run);
        let run_error = run_store
            .activate_prepared(run_prepared, run_prepared.commit_horizon())
            .expect_err("corrupt prepared run must not activate");
        assert!(run_error.to_string().contains("checksum mismatch"));
        assert_eq!(run_store.get(&run_key).expect("read corrupt run"), None);
        assert!(
            manifest::list_manifest_files(run_root.path())
                .expect("list run manifests")
                .is_empty()
        );
    }

    #[test]
    fn external_commit_horizon_discards_complete_orphan_suffix() {
        let root = tempdir().expect("temporary append store");
        let target = key(1);
        let orphan_only = key(9);
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(target, b"committed-zero")])
            .expect("append frame zero");
        store
            .append(&[put(target, b"committed-one")])
            .expect("append frame one");
        let committed = store.last_frame_receipt().expect("committed frame receipt");
        store
            .append(&[
                put(target, b"orphan-value"),
                put(orphan_only, b"orphan-only"),
            ])
            .expect("append complete orphan frame");
        drop(store);

        let mut reopened = PackStore::open_at_commit_horizon(
            root.path(),
            1024 * 1024,
            Some(PackCommitHorizon {
                epoch: committed.epoch,
                payload_sha256: committed.payload_sha256,
            }),
        )
        .expect("recover to external commit marker");
        assert_eq!(reopened.open_validation().frames, 2);
        assert_eq!(reopened.last_frame_receipt(), Some(committed));
        assert_eq!(
            reopened.get(&target).expect("read committed value"),
            Some(b"committed-one".to_vec())
        );
        assert_eq!(
            reopened.get(&orphan_only).expect("read discarded key"),
            None
        );

        reopened
            .append(&[put(target, b"replacement-two")])
            .expect("append replacement frame");
        assert_eq!(
            reopened
                .last_frame_receipt()
                .expect("replacement receipt")
                .epoch,
            2
        );
        assert_eq!(
            reopened.get(&target).expect("read replacement value"),
            Some(b"replacement-two".to_vec())
        );
    }

    #[test]
    fn external_commit_horizon_rejects_missing_or_checksum_mismatched_frame() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), 1024 * 1024).expect("create store");
        store
            .append(&[put(key(1), b"committed")])
            .expect("append committed frame");
        let receipt = store.last_frame_receipt().expect("frame receipt");
        drop(store);

        let mut wrong_checksum = receipt.payload_sha256;
        wrong_checksum[0] ^= 0x80;
        let checksum_error = PackStore::open_at_commit_horizon(
            root.path(),
            1024 * 1024,
            Some(PackCommitHorizon {
                epoch: receipt.epoch,
                payload_sha256: wrong_checksum,
            }),
        )
        .err()
        .expect("checksum mismatch must fail");
        assert!(checksum_error.to_string().contains("checksum"));

        let missing_error = PackStore::open_at_commit_horizon(
            root.path(),
            1024 * 1024,
            Some(PackCommitHorizon {
                epoch: receipt.epoch + 1,
                payload_sha256: receipt.payload_sha256,
            }),
        )
        .err()
        .expect("missing committed frame must fail");
        assert!(missing_error.to_string().contains("only 1 complete frames"));

        let reopened = PackStore::open_at_commit_horizon(
            root.path(),
            1024 * 1024,
            Some(PackCommitHorizon {
                epoch: receipt.epoch,
                payload_sha256: receipt.payload_sha256,
            }),
        )
        .expect("valid marker remains recoverable");
        assert_eq!(reopened.open_validation().frames, 1);
    }
}

//! # neo-state-packs
//!
//! Append-only, checksummed node-pack persistence for Neo StateService MPT
//! rows, extracted from the `append_frames_immutable_sorted_runs_v3`
//! benchmark prototype.
//!
//! ## On-disk layout
//!
//! - `frames.pack`: one ordered operation stream per commit epoch. Every
//!   frame has a 72-byte versioned header (`N3PACK01`, epoch, row count,
//!   payload length, SHA-256 payload checksum) followed by rows of
//!   `key(33) || kind(1) || value_len(4 LE) || value`. A frame becomes
//!   visible only when a manifest generation referencing its index run is
//!   published; anything past the committed prefix is torn/orphaned tail and
//!   is truncated on open.
//! - `runs/`: immutable sorted index runs (`N3IDXR01`): v3 level-0 runs use an
//!   xor16 filter, while streaming compaction emits physical v4 runs with a
//!   mmap-backed blocked Bloom filter. Both use a 192-byte tagged header,
//!   domain-separated structure SHA-256, sparse 16-byte fence keys every 64
//!   records, and fixed 50-byte records
//!   (`key(33) || sequence(4) || value_offset(8) || value_len(4) ||
//!   tombstone(1)`). Records point into `frames.pack`; compaction merges
//!   runs newest-epoch-wins without rewriting payloads.
//! - `manifest-*.man`: immutable manifest generations (`N3MANI01`) listing
//!   the live runs. Publication is atomic (write `.tmp`, sync, rename,
//!   directory sync), so a generation is the local visibility activation
//!   point of a frame.
//! - `writer.lock`: a kernel-held advisory lease acquired before recovery or
//!   mutation and retained for the complete [`PackStore`] lifetime. It keeps
//!   two node processes from repairing or appending the same pack concurrently.
//!
//! Snapshot handles pin one manifest generation through an explicit lease;
//! superseded runs and manifests are reclaimed only by an explicit
//! [`PackStore::gc`]. Missing or corrupt derived index runs are rebuilt from
//! committed frames on open.
//!
//! ## Two-phase publication
//!
//! [`PackStore::prepare_append`] durably syncs a frame and immutable run but
//! leaves the manifest and live view unchanged. Shadow callers may commit an
//! external [`PackCommitHorizon`] and then invoke
//! [`PackStore::activate_prepared`]. Production coordinated callers use
//! [`PackStore::seal_prepared`] first: it completes validation, publishes a
//! provisional manifest, and pins the resulting snapshot before the external
//! commit. After that commit the caller only swaps the already-created
//! snapshot into its read view. A sealed manifest is not canonical authority;
//! [`PackStore::open_at_commit_horizon`] discards it when restart selects an
//! earlier horizon. The external horizon remains the canonical decision, and
//! manifests and runs remain rebuildable visibility aids.
//!
//! ## Boundary
//!
//! This crate owns the on-disk format and recovery only. It does not depend
//! on MPT mutation logic, MDBX, or node composition: callers hand it
//! versioned raw key/value operations. The shadow dual-write adapter lives
//! in [`shadow`]; the MDBX commit-authority marker it produces is defined
//! there as well ([`shadow::ShadowHighWaterRecord`]).
//!
//! ## Contents
//!
//! - `authority`: mandatory canonical high-water marker encoding.
//! - `engine`: generic append-frame storage and derived indexes.
//! - `shadow`: StateService node filtering and MDBX marker records.

pub mod authority;
mod engine;
pub mod shadow;

pub use engine::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, CheckpointNamespaceEvidence, CompactionDebt,
    CompactionStats, GcStats, OpenValidation, PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION,
    PACK_INDEX_RUN_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION, PackCommitHorizon,
    PackCompactionPlan, PackFrameReceipt, PackIndexScrubStats, PackMaterializedViewEvidence,
    PackScrubStats, PackStore, PackStoreError, PackStoreOptions, PreparedAppend,
    PreparedPackCompaction, SealedAppend, Snapshot,
};

/// Byte length of one pack key (the StateService `0xf0 || node_hash` node
/// key). The engine treats keys as opaque fixed-size arrays; namespace
/// filtering is the caller's concern (see [`shadow`]).
pub const PACK_KEY_BYTES: usize = 33;

/// One versioned key/value operation staged for a single frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOperation {
    /// Fixed-size raw key bytes.
    pub key: [u8; PACK_KEY_BYTES],
    /// Put or tombstone operation applied to `key`.
    pub kind: PackOpKind,
}

/// Operation kind carried by a [`PackOperation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackOpKind {
    /// Insert or replace the value stored under the key.
    Put(Vec<u8>),
    /// Remove the value stored under the key. Tombstones persist until
    /// compaction drops every version they mask.
    Tombstone,
}

/// Per-frame stage timings and counters produced by
/// [`PackStore::prepare_append`] and returned by [`PackStore::append`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PackStageTotals {
    /// Time spent writing append-frame bytes.
    pub append_write_ns: u64,
    /// Time spent syncing the append pack.
    pub pack_sync_ns: u64,
    /// Time spent writing the immutable index run.
    pub index_write_ns: u64,
    /// Time spent syncing the immutable index run.
    pub index_sync_ns: u64,
    /// Time spent syncing the index-run directory.
    pub directory_sync_ns: u64,
    /// Durable frames written (always 1 for a successful prepare or append).
    pub frames: u64,
    /// Index records written in the frame's immutable run.
    pub index_entries: u64,
}

impl PackStageTotals {
    /// Accumulates another frame's totals into this one.
    pub fn merge(&mut self, other: Self) {
        self.append_write_ns = self.append_write_ns.saturating_add(other.append_write_ns);
        self.pack_sync_ns = self.pack_sync_ns.saturating_add(other.pack_sync_ns);
        self.index_write_ns = self.index_write_ns.saturating_add(other.index_write_ns);
        self.index_sync_ns = self.index_sync_ns.saturating_add(other.index_sync_ns);
        self.directory_sync_ns = self
            .directory_sync_ns
            .saturating_add(other.directory_sync_ns);
        self.frames = self.frames.saturating_add(other.frames);
        self.index_entries = self.index_entries.saturating_add(other.index_entries);
    }
}

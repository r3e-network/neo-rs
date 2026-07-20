//! Cold-first append lifecycle values.

use std::sync::Arc;

use crate::PackStageTotals;

use super::super::Snapshot;

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
    pub(in crate::engine::store) receipt: PackFrameReceipt,
    pub(in crate::engine::store) stage_totals: PackStageTotals,
    pub(in crate::engine::store) store_instance_id: u64,
    pub(in crate::engine::store) serial: u64,
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
/// [`crate::PackStore::seal_prepared`] creates and pins the snapshot before returning,
/// so after the caller commits [`Self::commit_horizon`] its only required
/// in-process action is to swap [`Self::into_snapshot`] into the node's read
/// view. The manifest on disk is provisional until that external commit. If
/// the commit fails, the writer must be dropped and reopened through the prior
/// horizon; [`crate::PackStore::open_at_commit_horizon`] then discards this suffix.
#[must_use = "a sealed append must be committed externally or discarded by reopening at the prior horizon"]
pub struct SealedAppend {
    pub(in crate::engine::store) commit_horizon: PackCommitHorizon,
    pub(in crate::engine::store) snapshot: Arc<Snapshot>,
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

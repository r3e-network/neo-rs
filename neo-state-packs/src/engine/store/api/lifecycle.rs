//! Cold-first append lifecycle values.

use std::sync::Arc;

use crate::PackStageTotals;

use super::super::Snapshot;
use super::identity::{PackPosition, PackSegmentId};

/// Immutable block and StateService-root context authenticated by one frame.
///
/// Pack storage validates this context locally. It deliberately does not
/// require adjacent frames to be contiguous because checkpoint chunks may
/// repeat one source context and metadata-only StateService commits may occur
/// between node-bearing frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFrameContext {
    /// Lowest block index represented by the frame's operation window.
    pub block_start: u32,
    /// Highest block index represented by the frame's operation window.
    pub block_end: u32,
    /// StateService root before applying the represented operation window.
    pub previous_root: [u8; 32],
    /// StateService root after applying the represented operation window.
    pub resulting_root: [u8; 32],
}

impl PackFrameContext {
    /// Creates an immutable frame context.
    pub const fn new(
        block_start: u32,
        block_end: u32,
        previous_root: [u8; 32],
        resulting_root: [u8; 32],
    ) -> Self {
        Self {
            block_start,
            block_end,
            previous_root,
            resulting_root,
        }
    }
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
    /// Segment containing the complete frame.
    pub segment_id: PackSegmentId,
    /// Segment-relative byte offset of the frame header.
    pub frame_start: u64,
    /// Segment-relative byte offset one past the complete frame footer.
    pub frame_end: u64,
    /// Authenticated block and root context carried by the frame.
    pub context: PackFrameContext,
    /// Number of operations encoded in the frame.
    pub rows: u64,
    /// Sorted fixed-width row-metadata bytes.
    pub metadata_bytes: u64,
    /// Put-value payload bytes. Tombstones contribute no value bytes.
    pub value_bytes: u64,
    /// Domain-separated digest of the fixed header.
    ///
    /// The header transitively authenticates the metadata and value sections
    /// through their independent domain-separated digests. The footer binds
    /// this digest to the epoch and exact complete-frame length.
    pub frame_sha256: [u8; 32],
}

impl PackFrameReceipt {
    /// Returns the segment-relative start position of this frame.
    pub const fn start_position(self) -> PackPosition {
        PackPosition::new(self.segment_id, self.frame_start)
    }

    /// Returns the segment-relative end position of this frame.
    pub const fn end_position(self) -> PackPosition {
        PackPosition::new(self.segment_id, self.frame_end)
    }
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
            segment_id: self.receipt.segment_id,
            frame_end: self.receipt.frame_end,
            context: self.receipt.context,
            frame_sha256: self.receipt.frame_sha256,
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
    /// Segment containing the canonically committed frame.
    pub segment_id: PackSegmentId,
    /// Segment-relative byte offset immediately after the committed frame.
    ///
    /// Binding the external marker to both the checksum and placement rejects
    /// a structurally valid frame chain whose canonical high-water record was
    /// corrupted or belongs to another pack layout.
    pub frame_end: u64,
    /// Immutable block and root context authenticated by that frame.
    pub context: PackFrameContext,
    /// Domain-separated digest of that frame's authenticated header.
    pub frame_sha256: [u8; 32],
}

impl PackCommitHorizon {
    /// Returns the exact segment-relative end selected by this horizon.
    pub const fn end_position(self) -> PackPosition {
        PackPosition::new(self.segment_id, self.frame_end)
    }
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

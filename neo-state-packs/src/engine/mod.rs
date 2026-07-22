//! # Pack engine
//!
//! Append-frame encoding, immutable indexes, manifest publication, recovery,
//! compaction, snapshots, and bounded read APIs.
//!
//! ## Boundary
//!
//! This module owns fixed-width `0xf0 || node_hash` pack mechanics and validates
//! caller-supplied block/root context. It does not interpret MPT values or know
//! about MDBX markers, contracts, or execution.
//!
//! ## Contents
//!
//! - `filter`: verified run membership filters.
//! - `manifest`: immutable visibility generations.
//! - `mmap`: the scoped read-only mapping implementation.
//! - `store`: append, recovery, lookup, compaction, leases, and GC.

mod failpoint;
mod filter;
mod manifest;
mod merge;
mod metrics;
#[allow(unsafe_code)]
mod mmap;
mod store;
pub(crate) use store::initial_segment_exists;

pub use metrics::{CompactionDebt, CompactionStats, GcStats, PackMetrics, ReadMetrics};

pub use manifest::PACK_MANIFEST_FORMAT_VERSION;
pub use store::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, CheckpointNamespaceEvidence, OpenValidation,
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_INDEX_RUN_FORMAT_VERSION,
    PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackCommitHorizon, PackCompactionPlan,
    PackFrameBuilder, PackFrameContext, PackFrameReceipt, PackIndexScrubStats,
    PackMaterializedViewEvidence, PackPosition, PackScrubStats, PackSegmentId, PackStore,
    PackStoreArtifact, PackStoreConfig, PackStoreConfigError, PackStoreConfigField, PackStoreError,
    PackStoreErrorSource, PackStoreLimit, PackStoreOperation, PackStoreOptions, PackStoreResult,
    PreparedAppend, PreparedPackCompaction, SealedAppend, Snapshot,
};

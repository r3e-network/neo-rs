//! # Pack engine
//!
//! Append-frame encoding, immutable indexes, manifest publication, recovery,
//! compaction, snapshots, and bounded read APIs.
//!
//! ## Boundary
//!
//! This module owns generic fixed-key pack mechanics. It does not know about
//! StateService namespaces, MDBX markers, blocks, contracts, or execution.
//! Callers provide opaque versioned operations and an external commit horizon.
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
#[allow(unsafe_code)]
mod mmap;
mod store;

pub use manifest::PACK_MANIFEST_FORMAT_VERSION;
pub use store::{
    CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, CheckpointNamespaceEvidence, CompactionDebt,
    CompactionStats, GcStats, OpenValidation, PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION,
    PACK_INDEX_RUN_FORMAT_VERSION, PackCommitHorizon, PackCompactionPlan, PackFrameReceipt,
    PackIndexScrubStats, PackMaterializedViewEvidence, PackScrubStats, PackStore, PackStoreError,
    PackStoreOptions, PreparedAppend, PreparedPackCompaction, SealedAppend, Snapshot,
};

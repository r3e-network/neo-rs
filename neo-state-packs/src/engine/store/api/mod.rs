//! # Pack-store API
//!
//! ## Boundary
//!
//! This module owns caller-facing configuration, operational errors, and
//! append lifecycle values. Frame encoding, publication, recovery, and lookup
//! mechanics remain in sibling store modules.
//!
//! ## Contents
//!
//! - `config`: bounded physical read and compaction settings.
//! - `error`: typed operational failures callers may classify.
//! - `identity`: stable segment identities and positioned locations.
//! - `lifecycle`: receipts and handoff values for cold-first publication.

mod config;
mod error;
mod identity;
mod lifecycle;

pub use config::{PackStoreConfig, PackStoreConfigError, PackStoreConfigField, PackStoreOptions};
pub use error::{
    PackStoreArtifact, PackStoreError, PackStoreErrorSource, PackStoreLimit, PackStoreOperation,
    PackStoreResult,
};
pub use identity::{
    PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackPosition, PackSegmentId,
};
pub use lifecycle::{
    OpenValidation, PackCommitHorizon, PackFrameReceipt, PreparedAppend, SealedAppend,
};

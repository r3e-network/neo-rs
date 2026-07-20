//! Pack-store API data contracts.
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
//! - `lifecycle`: receipts and handoff values for cold-first publication.

mod config;
mod error;
mod lifecycle;

pub(crate) use config::CompactionConfig;
pub use config::PackStoreOptions;
pub(in crate::engine::store) use config::{validate_compaction_config, validate_store_options};
pub use error::PackStoreError;
pub use lifecycle::{
    OpenValidation, PackCommitHorizon, PackFrameReceipt, PreparedAppend, SealedAppend,
};

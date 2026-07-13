//! # neo-static-files
//!
//! Append-only, compressed storage for finalized opaque ledger records.
//!
//! ## Boundary
//!
//! This infrastructure crate owns the static-file format, height-addressed
//! segment rotation, genesis-first continuity checks, checksums, tail recovery,
//! exclusive writer lease, an MDBX-backed versioned offset index, strict
//! scrubbing, and key/value lookup.
//! It does not understand Neo block, transaction, VM, native-contract, or
//! state-root semantics; higher layers decide which immutable bytes enter each
//! finalized-height record. Archive frames are authoritative; the adjacent
//! MDBX index is derived and can be rebuilt from them.
//!
//! ## Contents
//!
//! - `archive`: Provider/factory API plus segmented append, indexed read,
//!   recovery, scrubbing, and ownership.
//! - `error`: Static-file-specific failures.
//! - `format`: Versioned frame encoding and validation.
//! - `record`: Opaque finalized-height records and rows.

mod archive;
mod error;
mod format;
mod record;

pub use archive::{
    StaticFileArchive, StaticFileArchiveFactory, StaticFileConfig, StaticFileOpenStats,
    StaticFileProvider, StaticFileProviderFactory,
};
pub use error::{StaticFileError, StaticFileResult};
pub use record::{StaticRecord, StaticRow};

#[cfg(test)]
mod tests;

//! # Static archive provider
//!
//! ## Boundary
//!
//! This module owns archive lifecycle, exclusive writer ownership, durable
//! append ordering, lookup, truncation, and recovery. Frame bytes and opaque
//! records are defined by sibling modules.
//!
//! ## Contents
//!
//! - `config`: Compression, cache, and resource limits.
//! - `factory`: Archive creation, indexed open, and suffix recovery.
//! - `index`: MDBX frame/row locations and strict archive scanning.
//! - `io`: Positioned file I/O and directory durability helpers.
//! - `lease`: Kernel-held single-writer exclusion.
//! - `provider`: Cloneable staged publication, lookup, truncate, and scrub
//!   capability.

use std::path::Path;

use crate::StaticFileResult;

mod config;
mod factory;
mod index;
mod io;
mod lease;
mod provider;

pub use config::StaticFileConfig;
pub use factory::{StaticFileArchiveFactory, StaticFileOpenStats};
pub use provider::StaticFileArchive;

/// Read capability for opaque records in a static-file archive.
pub trait StaticFileProvider: Clone + Send + Sync + 'static {
    /// Returns the highest complete archived height, or `None` when empty.
    fn tip(&self) -> Option<u32>;

    /// Returns the latest value stored under `key` at the archive tip.
    fn get(&self, key: &[u8]) -> StaticFileResult<Option<Vec<u8>>>;

    /// Returns the raw row keys captured in the archived frame at `height`.
    ///
    /// Implementations may satisfy this from frame metadata alone without
    /// decompressing frame payload bytes.
    fn frame_row_keys(&self, height: u32) -> StaticFileResult<Option<Vec<Vec<u8>>>>;

    /// Returns the latest archived height for each key in `keys`.
    ///
    /// The returned vector preserves input order and contains `None` for keys
    /// with no archived version.
    fn latest_heights_for_keys<K: AsRef<[u8]>>(
        &self,
        keys: &[K],
    ) -> StaticFileResult<Vec<Option<u32>>>;
}

/// Factory contract for concrete static-file providers.
pub trait StaticFileProviderFactory {
    /// Provider opened by this factory.
    type Provider: StaticFileProvider;

    /// Opens or creates the archive at `path`, repairing an incomplete tail.
    fn open(&self, path: &Path) -> StaticFileResult<Self::Provider>;
}

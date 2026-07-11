//! Static-file provider, factory, ownership, and crash-recovery facade.

use std::path::Path;

use crate::StaticFileResult;

mod config;
mod factory;
mod index;
mod io;
mod lease;
mod provider;

pub use config::StaticFileConfig;
pub use factory::StaticFileArchiveFactory;
pub use provider::StaticFileArchive;

/// Read capability for opaque records in a static-file archive.
pub trait StaticFileProvider: Clone + Send + Sync + 'static {
    /// Returns the highest complete archived height, or `None` when empty.
    fn tip(&self) -> Option<u32>;

    /// Returns the latest value stored under `key` at the archive tip.
    fn get(&self, key: &[u8]) -> StaticFileResult<Option<Vec<u8>>>;
}

/// Factory contract for concrete static-file providers.
pub trait StaticFileProviderFactory {
    /// Provider opened by this factory.
    type Provider: StaticFileProvider;

    /// Opens or creates the archive at `path`, repairing an incomplete tail.
    fn open(&self, path: &Path) -> StaticFileResult<Self::Provider>;
}

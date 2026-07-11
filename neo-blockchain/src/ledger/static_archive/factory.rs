//! Factory for the typed finalized Ledger archive.

use std::path::Path;

use neo_error::{CoreError, CoreResult};
use neo_static_files::{StaticFileArchiveFactory, StaticFileConfig, StaticFileProviderFactory};

use super::StaticLedgerArchive;

/// Opens protocol-aware Ledger archives over the protocol-blind static-file
/// engine.
#[derive(Clone, Debug, Default)]
pub struct StaticLedgerArchiveFactory {
    files: StaticFileArchiveFactory,
}

impl StaticLedgerArchiveFactory {
    /// Creates a factory with the supplied static-file policy.
    #[must_use]
    pub fn new(config: StaticFileConfig) -> Self {
        Self {
            files: StaticFileArchiveFactory::new(config),
        }
    }

    /// Opens and recovers the Ledger archive at `path`.
    pub fn open(&self, path: impl AsRef<Path>) -> CoreResult<StaticLedgerArchive> {
        self.files
            .open(path.as_ref())
            .map(StaticLedgerArchive::new)
            .map_err(|error| CoreError::io(format!("open static Ledger archive: {error}")))
    }
}

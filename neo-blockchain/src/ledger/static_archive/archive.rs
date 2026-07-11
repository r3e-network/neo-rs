//! Typed Ledger facade over the protocol-blind static-file provider.

use neo_error::{CoreError, CoreResult};
use neo_payloads::Block;
use neo_static_files::{StaticFileArchive, StaticFileProvider, StaticRecord};
use neo_storage::{CacheRead, DataCache, StorageKey};

use crate::ledger::ledger_provider::StaticLedgerProvider;

/// Append-only mirror of immutable native Ledger records.
#[derive(Clone, Debug)]
pub struct StaticLedgerArchive {
    files: StaticFileArchive,
}

impl StaticLedgerArchive {
    /// Wraps a protocol-blind static-file provider with Ledger semantics.
    #[must_use]
    pub const fn new(files: StaticFileArchive) -> Self {
        Self { files }
    }

    /// Returns a typed cold Ledger provider over this archive.
    #[must_use]
    pub fn provider(&self) -> StaticLedgerProvider {
        StaticLedgerProvider::new(self.clone())
    }

    /// Returns the underlying static-file provider.
    #[must_use]
    pub const fn files(&self) -> &StaticFileArchive {
        &self.files
    }

    /// Returns the highest archived block height.
    #[must_use]
    pub fn tip(&self) -> Option<u32> {
        self.files.tip()
    }

    /// Captures the exact finalized Ledger rows for `block` from `snapshot`.
    pub fn capture_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        block: &Block,
    ) -> CoreResult<StaticRecord> {
        super::capture::capture_block(snapshot, block)
    }

    /// Captures and durably appends one finalized block.
    pub fn append_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        block: &Block,
    ) -> CoreResult<()> {
        let record = self.capture_block(snapshot, block)?;
        self.append_records(vec![record])
    }

    /// Durably appends one contiguous batch with a single file sync.
    pub fn append_records(&self, records: Vec<StaticRecord>) -> CoreResult<()> {
        self.files
            .append_batch(records)
            .map_err(|error| static_file_error("append Ledger archive", error))
    }

    pub(crate) fn get(&self, key: &StorageKey) -> CoreResult<Option<Vec<u8>>> {
        self.files
            .get(key.as_bytes().as_ref())
            .map_err(|error| static_file_error("read Ledger archive", error))
    }

    pub(crate) fn truncate_after(&self, height: Option<u32>) -> CoreResult<()> {
        self.files
            .truncate_after(height)
            .map_err(|error| static_file_error("truncate Ledger archive", error))
    }
}

fn static_file_error(context: &'static str, error: neo_static_files::StaticFileError) -> CoreError {
    CoreError::io(format!("{context}: {error}"))
}

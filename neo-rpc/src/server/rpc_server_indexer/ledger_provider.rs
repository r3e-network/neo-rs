//! Ledger read capabilities for indexer RPC handlers.
//!
//! Indexer status needs only the current ledger height. Keeping that read
//! behind this local provider seam leaves the handler focused on response
//! assembly and keeps raw ledger provider construction in one place.

use crate::server::ledger_queries;
use neo_storage::persistence::DataCache;

/// Ledger capabilities required by indexer RPC handlers.
pub(super) trait IndexerLedgerProvider {
    /// Returns the current persisted ledger height, or `None` if the ledger tip
    /// is not available yet.
    fn ledger_height(&self, snapshot: &DataCache) -> Option<u32>;
}

/// Factory for indexer RPC ledger providers.
pub(super) trait IndexerLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: IndexerLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeIndexerLedgerProvider;

impl NativeIndexerLedgerProvider {
    /// Creates the production indexer ledger provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }
}

impl IndexerLedgerProvider for NativeIndexerLedgerProvider {
    fn ledger_height(&self, snapshot: &DataCache) -> Option<u32> {
        ledger_queries::current_index(snapshot).ok()
    }
}

/// Factory for production indexer ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeIndexerLedgerProviderFactory;

impl IndexerLedgerProviderFactory for NativeIndexerLedgerProviderFactory {
    type Provider = NativeIndexerLedgerProvider;

    fn provider(&self) -> Self::Provider {
        NativeIndexerLedgerProvider::new()
    }
}

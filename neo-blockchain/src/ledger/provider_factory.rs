//! # neo-blockchain::ledger::provider_factory
//!
//! Factory abstractions for ledger read providers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. It owns provider construction for
//! ledger read views, but it does not write ledger records, prune history, or
//! decide whether cold static files are enabled in a node configuration.
//!
//! ## Contents
//!
//! - `LedgerProviderFactory`: common factory trait for ledger read providers.
//! - `StorageLedgerProviderFactory`: creates hot providers over native Ledger
//!   records in a `DataCache`.
//! - `HotColdLedgerProviderFactory`: composes hot and cold provider factories.
//!
//! Provider factories give callers a consistent way to obtain a read-only view
//! of hot, cold, or composed ledger storage without depending on the concrete
//! backing layout.

use super::ledger_provider::{BlockProvider, StorageLedgerProvider, TxProvider};
use super::static_archive::{HotColdLedgerProvider, StaticLedgerArchive};
use neo_error::CoreResult;
use neo_storage::DataCache;
use std::sync::Arc;

/// Factory for read-only ledger providers.
pub trait LedgerProviderFactory {
    /// Provider type returned for a view borrowed from this factory.
    type Provider<'a>: BlockProvider + TxProvider + 'a
    where
        Self: 'a;

    /// Opens the latest canonical ledger view exposed by this factory.
    fn latest(&self) -> CoreResult<Self::Provider<'_>>;
}

/// Factory for storage-backed hot ledger providers.
pub struct StorageLedgerProviderFactory {
    snapshot: Arc<DataCache>,
}

impl StorageLedgerProviderFactory {
    /// Creates a factory over a shared storage snapshot.
    #[must_use]
    pub const fn new(snapshot: Arc<DataCache>) -> Self {
        Self { snapshot }
    }

    /// Returns the snapshot used by providers created from this factory.
    #[must_use]
    pub fn snapshot(&self) -> &Arc<DataCache> {
        &self.snapshot
    }
}

impl LedgerProviderFactory for StorageLedgerProviderFactory {
    type Provider<'a>
        = StorageLedgerProvider<'a>
    where
        Self: 'a;

    fn latest(&self) -> CoreResult<Self::Provider<'_>> {
        Ok(StorageLedgerProvider::new(self.snapshot.as_ref()))
    }
}

impl LedgerProviderFactory for StaticLedgerArchive {
    type Provider<'a>
        = &'a StaticLedgerArchive
    where
        Self: 'a;

    fn latest(&self) -> CoreResult<Self::Provider<'_>> {
        Ok(self)
    }
}

/// Factory that composes a hot ledger provider with a cold ledger provider.
pub struct HotColdLedgerProviderFactory<H, C> {
    hot: H,
    cold: C,
}

impl<H, C> HotColdLedgerProviderFactory<H, C> {
    /// Creates a hot/cold provider factory.
    #[must_use]
    pub const fn new(hot: H, cold: C) -> Self {
        Self { hot, cold }
    }
}

impl<H, C> LedgerProviderFactory for HotColdLedgerProviderFactory<H, C>
where
    H: LedgerProviderFactory,
    C: LedgerProviderFactory,
{
    type Provider<'a>
        = HotColdLedgerProvider<H::Provider<'a>, C::Provider<'a>>
    where
        Self: 'a;

    fn latest(&self) -> CoreResult<Self::Provider<'_>> {
        Ok(HotColdLedgerProvider::new(
            self.hot.latest()?,
            self.cold.latest()?,
        ))
    }
}

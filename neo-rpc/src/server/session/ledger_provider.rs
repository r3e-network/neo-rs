//! Ledger read capabilities for RPC invocation sessions.
//!
//! Session dummy-block construction needs the current persisted header so it can
//! synthesize the C#-compatible persisting block used by stateless invokes. This
//! seam keeps those ledger reads behind a local capability trait instead of
//! letting dummy-block construction adapt directly to storage providers.

use neo_blockchain::{
    BlockProvider, ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory,
    LedgerProviderFactory,
};
use neo_error::CoreResult;
use neo_payloads::Header;
use neo_primitives::UInt256;
use neo_storage::persistence::{CacheRead, DataCache};

const SESSION_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

/// Ledger capabilities required by RPC session construction.
pub(super) trait SessionLedgerProvider {
    /// Returns the current persisted block hash and header.
    fn current_header<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<(UInt256, Header)>>;
}

/// Factory for RPC session ledger providers.
pub(super) trait SessionLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: SessionLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSessionLedgerProvider;

impl NativeSessionLedgerProvider {
    /// Creates the production session ledger provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }
}

impl SessionLedgerProvider for NativeSessionLedgerProvider {
    fn current_header<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<(UInt256, Header)>> {
        let provider = SESSION_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        let current_hash = provider.current_hash()?;
        Ok(provider
            .header_by_hash(&current_hash)?
            .map(|header| (current_hash, header)))
    }
}

/// Factory for production RPC session ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSessionLedgerProviderFactory;

impl SessionLedgerProviderFactory for NativeSessionLedgerProviderFactory {
    type Provider = NativeSessionLedgerProvider;

    fn provider(&self) -> Self::Provider {
        NativeSessionLedgerProvider::new()
    }
}

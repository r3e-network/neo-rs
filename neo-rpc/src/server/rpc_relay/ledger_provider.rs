//! Ledger read capabilities for RPC relay preflight.
//!
//! Block relay preflight needs only the current persisted height. Keeping that
//! read behind this local provider seam leaves relay orchestration focused on
//! C#-compatible block classification and blockchain-service submission.

use crate::server::ledger_queries;
use neo_storage::persistence::{CacheRead, DataCache};

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;

/// Ledger capabilities required by RPC relay preflight.
pub(super) trait RelayLedgerProvider {
    /// Returns the current persisted ledger height.
    fn current_height<B: CacheRead>(&self, snapshot: &DataCache<B>) -> Result<u32, RpcException>;
}

/// Factory for relay ledger providers.
pub(super) trait RelayLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: RelayLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeRelayLedgerProvider;

impl NativeRelayLedgerProvider {
    /// Creates the production relay ledger provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }
}

impl RelayLedgerProvider for NativeRelayLedgerProvider {
    fn current_height<B: CacheRead>(&self, snapshot: &DataCache<B>) -> Result<u32, RpcException> {
        ledger_queries::current_index(snapshot).map_err(|err| internal_error(err.to_string()))
    }
}

/// Factory for production relay ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeRelayLedgerProviderFactory;

impl RelayLedgerProviderFactory for NativeRelayLedgerProviderFactory {
    type Provider = NativeRelayLedgerProvider;

    fn provider(&self) -> Self::Provider {
        NativeRelayLedgerProvider::new()
    }
}

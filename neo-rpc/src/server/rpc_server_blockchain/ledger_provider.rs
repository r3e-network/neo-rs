//! Ledger read capabilities for blockchain RPC handlers.
//!
//! Blockchain RPC handlers should assemble JSON-RPC responses, not construct
//! lower-level ledger providers inline. This seam keeps the canonical storage
//! provider behind a local capability trait so each handler depends only on the
//! ledger reads it actually needs.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use neo_blockchain::{ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory};
use neo_storage::persistence::DataCache;

/// Ledger capabilities required by blockchain RPC handlers.
pub(super) trait BlockchainLedgerProvider {
    /// Returns the current persisted ledger height.
    fn current_height(&self, snapshot: &DataCache) -> Result<u32, RpcException>;
}

/// Factory for blockchain RPC ledger providers.
pub(super) trait BlockchainLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: BlockchainLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeBlockchainLedgerProvider;

impl NativeBlockchainLedgerProvider {
    /// Creates the production blockchain ledger provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }
}

impl BlockchainLedgerProvider for NativeBlockchainLedgerProvider {
    fn current_height(&self, snapshot: &DataCache) -> Result<u32, RpcException> {
        StorageLedgerProviderFactory
            .provider(snapshot)
            .current_index()
            .map_err(|err| internal_error(err.to_string()))
    }
}

/// Factory for production blockchain ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeBlockchainLedgerProviderFactory;

impl BlockchainLedgerProviderFactory for NativeBlockchainLedgerProviderFactory {
    type Provider = NativeBlockchainLedgerProvider;

    fn provider(&self) -> Self::Provider {
        NativeBlockchainLedgerProvider::new()
    }
}

//! Ledger read capabilities for blockchain RPC handlers.
//!
//! Blockchain RPC handlers should assemble JSON-RPC responses, not construct
//! lower-level ledger providers inline. This seam keeps the canonical storage
//! provider behind a local capability trait so each handler depends only on the
//! ledger reads it actually needs.

use crate::server::ledger_queries;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use neo_blockchain::{LedgerProviderFactory, TransactionStateProvider};
use neo_payloads::TransactionState;
use neo_primitives::UInt256;
use neo_storage::persistence::{CacheRead, DataCache};

/// Ledger capabilities required by blockchain RPC handlers.
pub(super) trait BlockchainLedgerProvider {
    /// Returns the current persisted ledger height.
    fn current_height<B: CacheRead>(&self, snapshot: &DataCache<B>) -> Result<u32, RpcException>;

    /// Returns the persisted transaction-state record for `hash`, including
    /// conflict stubs.
    fn transaction_state_by_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> Result<Option<TransactionState>, RpcException>;
}

/// Factory for blockchain RPC ledger providers.
pub(super) trait BlockchainLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: BlockchainLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeBlockchainLedgerProvider<'a, F> {
    ledger_provider_factory: &'a F,
}

impl<'a, F> NativeBlockchainLedgerProvider<'a, F> {
    /// Creates the production blockchain ledger provider.
    #[must_use]
    pub(super) const fn new(ledger_provider_factory: &'a F) -> Self {
        Self {
            ledger_provider_factory,
        }
    }
}

impl<F> BlockchainLedgerProvider for NativeBlockchainLedgerProvider<'_, F>
where
    F: LedgerProviderFactory,
{
    fn current_height<B: CacheRead>(&self, snapshot: &DataCache<B>) -> Result<u32, RpcException> {
        ledger_queries::current_index(snapshot).map_err(|err| internal_error(err.to_string()))
    }

    fn transaction_state_by_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> Result<Option<TransactionState>, RpcException> {
        self.ledger_provider_factory
            .provider(snapshot)
            .transaction_state_by_hash(hash)
            .map_err(|err| internal_error(err.to_string()))
    }
}

/// Factory for production blockchain ledger providers.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeBlockchainLedgerProviderFactory<'a, F> {
    ledger_provider_factory: &'a F,
}

impl<'a, F> NativeBlockchainLedgerProviderFactory<'a, F> {
    /// Adapts the node-composed Ledger provider factory to this RPC domain.
    #[must_use]
    pub(super) const fn new(ledger_provider_factory: &'a F) -> Self {
        Self {
            ledger_provider_factory,
        }
    }
}

impl<'a, F> BlockchainLedgerProviderFactory for NativeBlockchainLedgerProviderFactory<'a, F>
where
    F: LedgerProviderFactory,
{
    type Provider = NativeBlockchainLedgerProvider<'a, F>;

    fn provider(&self) -> Self::Provider {
        NativeBlockchainLedgerProvider::new(self.ledger_provider_factory)
    }
}

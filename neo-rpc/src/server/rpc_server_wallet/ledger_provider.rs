//! Ledger read capabilities for wallet RPC handlers.
//!
//! Wallet handlers should build wallet transactions and project wallet-domain
//! errors, not construct lower-level ledger providers inline. This seam keeps
//! transaction-state reads behind a local capability trait so transfer flows
//! depend only on the ledger records they actually need.

use neo_blockchain::{LedgerProviderFactory, TransactionStateProvider};
use neo_payloads::TransactionState;
use neo_primitives::UInt256;
use neo_storage::persistence::{CacheRead, DataCache};

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;

/// Ledger capabilities required by wallet RPC handlers.
pub(super) trait WalletLedgerProvider {
    /// Returns the persisted transaction-state record for `hash`, including
    /// conflict stubs.
    fn transaction_state_by_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> Result<Option<TransactionState>, RpcException>;
}

/// Factory for wallet RPC ledger providers.
pub(super) trait WalletLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider: WalletLedgerProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical ledger storage records.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeWalletLedgerProvider<'a, F> {
    ledger_provider_factory: &'a F,
}

impl<'a, F> NativeWalletLedgerProvider<'a, F> {
    /// Creates the production wallet ledger provider.
    #[must_use]
    pub(super) const fn new(ledger_provider_factory: &'a F) -> Self {
        Self {
            ledger_provider_factory,
        }
    }
}

impl<F> WalletLedgerProvider for NativeWalletLedgerProvider<'_, F>
where
    F: LedgerProviderFactory,
{
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

/// Factory for production wallet RPC ledger providers.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeWalletLedgerProviderFactory<'a, F> {
    ledger_provider_factory: &'a F,
}

impl<'a, F> NativeWalletLedgerProviderFactory<'a, F> {
    /// Adapts the node-composed Ledger provider factory to wallet RPC reads.
    #[must_use]
    pub(super) const fn new(ledger_provider_factory: &'a F) -> Self {
        Self {
            ledger_provider_factory,
        }
    }
}

impl<'a, F> WalletLedgerProviderFactory for NativeWalletLedgerProviderFactory<'a, F>
where
    F: LedgerProviderFactory,
{
    type Provider = NativeWalletLedgerProvider<'a, F>;

    fn provider(&self) -> Self::Provider {
        NativeWalletLedgerProvider::new(self.ledger_provider_factory)
    }
}

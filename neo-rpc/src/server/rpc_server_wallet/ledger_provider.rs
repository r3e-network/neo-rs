//! Ledger read capabilities for wallet RPC handlers.
//!
//! Wallet handlers should build wallet transactions and project wallet-domain
//! errors, not construct lower-level ledger providers inline. This seam keeps
//! transaction-state reads behind a local capability trait so transfer flows
//! depend only on the ledger records they actually need.

use neo_blockchain::{
    LedgerProviderFactory, StorageLedgerProviderFactory, TransactionStateProvider,
};
use neo_payloads::TransactionState;
use neo_primitives::UInt256;
use neo_storage::persistence::DataCache;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;

/// Ledger capabilities required by wallet RPC handlers.
pub(super) trait WalletLedgerProvider {
    /// Returns the persisted transaction-state record for `hash`, including
    /// conflict stubs.
    fn transaction_state_by_hash(
        &self,
        snapshot: &DataCache,
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
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletLedgerProvider;

impl NativeWalletLedgerProvider {
    /// Creates the production wallet ledger provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }
}

impl WalletLedgerProvider for NativeWalletLedgerProvider {
    fn transaction_state_by_hash(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
    ) -> Result<Option<TransactionState>, RpcException> {
        StorageLedgerProviderFactory
            .provider(snapshot)
            .transaction_state_by_hash(hash)
            .map_err(|err| internal_error(err.to_string()))
    }
}

/// Factory for production wallet RPC ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletLedgerProviderFactory;

impl WalletLedgerProviderFactory for NativeWalletLedgerProviderFactory {
    type Provider = NativeWalletLedgerProvider;

    fn provider(&self) -> Self::Provider {
        NativeWalletLedgerProvider::new()
    }
}

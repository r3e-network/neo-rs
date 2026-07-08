//! Transaction-admission read capabilities for composition-root helpers.
//!
//! The composition root wires runtime services and should depend on narrow
//! ledger/native capabilities instead of constructing storage ledger providers
//! or native contracts inside helper flows. This module owns those local
//! transaction-admission provider seams.

use neo_blockchain::{
    LedgerProviderFactory, StorageLedgerProviderFactory, TransactionStateProvider, TxProvider,
};
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;

/// Ledger capabilities required by transaction admission routing.
pub(super) trait TxAdmissionLedgerProvider {
    /// Returns whether `hash` is already persisted in the ledger.
    fn contains_transaction(&self, hash: &UInt256) -> CoreResult<bool>;

    /// Returns whether `hash` conflicts with a traceable on-chain transaction.
    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool>;
}

/// Factory for transaction-admission ledger providers.
pub(super) trait TxAdmissionLedgerProviderFactory {
    /// Provider returned by this factory.
    type Provider<'a>: TxAdmissionLedgerProvider
    where
        Self: 'a;

    /// Creates a provider instance over `snapshot`.
    fn provider<'a>(&self, snapshot: &'a DataCache) -> Self::Provider<'a>;
}

/// Production transaction-admission ledger provider over a storage snapshot.
pub(super) struct NativeTxAdmissionLedgerProvider<'a> {
    snapshot: &'a DataCache,
}

impl<'a> NativeTxAdmissionLedgerProvider<'a> {
    /// Creates a provider backed by the canonical storage ledger provider.
    #[must_use]
    pub(super) const fn new(snapshot: &'a DataCache) -> Self {
        Self { snapshot }
    }
}

impl TxAdmissionLedgerProvider for NativeTxAdmissionLedgerProvider<'_> {
    fn contains_transaction(&self, hash: &UInt256) -> CoreResult<bool> {
        StorageLedgerProviderFactory
            .provider(self.snapshot)
            .contains_transaction(hash)
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        StorageLedgerProviderFactory
            .provider(self.snapshot)
            .contains_conflict_hash(hash, signers, max_traceable_blocks)
    }
}

/// Factory for production transaction-admission ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionLedgerProviderFactory;

impl TxAdmissionLedgerProviderFactory for NativeTxAdmissionLedgerProviderFactory {
    type Provider<'a> = NativeTxAdmissionLedgerProvider<'a>;

    fn provider<'a>(&self, snapshot: &'a DataCache) -> Self::Provider<'a> {
        NativeTxAdmissionLedgerProvider::new(snapshot)
    }
}

/// Native-contract capabilities required by transaction admission routing.
pub(super) trait TxAdmissionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Factory for transaction-admission native providers.
pub(super) trait TxAdmissionNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: TxAdmissionNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionProvider {
    policy: PolicyContract,
}

impl NativeTxAdmissionProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl TxAdmissionNativeProvider for NativeTxAdmissionProvider {
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_traceable_blocks_snapshot(snapshot, settings)
    }
}

/// Factory for production transaction-admission native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionProviderFactory;

impl TxAdmissionNativeProviderFactory for NativeTxAdmissionProviderFactory {
    type Provider = NativeTxAdmissionProvider;

    fn provider(&self) -> Self::Provider {
        NativeTxAdmissionProvider::new()
    }
}

//! Transaction-admission read capabilities for composition-root helpers.
//!
//! The composition root wires runtime services and should depend on narrow
//! ledger/native capabilities instead of constructing storage ledger providers
//! or native contracts inside helper flows. This module owns those local
//! transaction-admission provider seams.

use neo_blockchain::{
    EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
    TransactionStateProvider, TxProvider,
};
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};

const TX_ADMISSION_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

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
    type Provider<'a, B>: TxAdmissionLedgerProvider
    where
        Self: 'a,
        B: CacheRead + 'a;

    /// Creates a provider instance over `snapshot`.
    fn provider<'a, B>(&self, snapshot: &'a DataCache<B>) -> Self::Provider<'a, B>
    where
        B: CacheRead;
}

/// Production transaction-admission ledger provider over a storage snapshot.
pub(super) struct NativeTxAdmissionLedgerProvider<'a, B>
where
    B: CacheRead,
{
    snapshot: &'a DataCache<B>,
}

impl<'a, B> NativeTxAdmissionLedgerProvider<'a, B>
where
    B: CacheRead,
{
    /// Creates a provider backed by the canonical storage ledger provider.
    #[must_use]
    pub(super) const fn new(snapshot: &'a DataCache<B>) -> Self {
        Self { snapshot }
    }
}

impl<B> TxAdmissionLedgerProvider for NativeTxAdmissionLedgerProvider<'_, B>
where
    B: CacheRead,
{
    fn contains_transaction(&self, hash: &UInt256) -> CoreResult<bool> {
        TX_ADMISSION_LEDGER_PROVIDER_FACTORY
            .provider(self.snapshot)
            .contains_transaction(hash)
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        TX_ADMISSION_LEDGER_PROVIDER_FACTORY
            .provider(self.snapshot)
            .contains_conflict_hash(hash, signers, max_traceable_blocks)
    }
}

/// Factory for production transaction-admission ledger providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionLedgerProviderFactory;

impl TxAdmissionLedgerProviderFactory for NativeTxAdmissionLedgerProviderFactory {
    type Provider<'a, B>
        = NativeTxAdmissionLedgerProvider<'a, B>
    where
        B: CacheRead + 'a;

    fn provider<'a, B>(&self, snapshot: &'a DataCache<B>) -> Self::Provider<'a, B>
    where
        B: CacheRead,
    {
        NativeTxAdmissionLedgerProvider::new(snapshot)
    }
}

/// Native-contract capabilities required by transaction admission routing.
pub(super) trait TxAdmissionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks<B>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>
    where
        B: CacheRead;
}

/// Adapter from the node-composed native-contract provider to the transaction
/// admission Policy read capability.
#[derive(Clone)]
pub(super) struct NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    native_contract_provider: Arc<P>,
}

impl<P> NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the node's composition-root native provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }
}

impl<P> std::fmt::Debug for NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTxAdmissionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> TxAdmissionNativeProvider for NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn max_traceable_blocks<B>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>
    where
        B: CacheRead,
    {
        self.provider().max_traceable_blocks(snapshot, settings)
    }
}

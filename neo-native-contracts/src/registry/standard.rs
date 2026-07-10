//! Typed handles for Neo's standard native-contract set.
//!
//! The standard native contracts are a closed protocol set. A concrete enum
//! keeps provider lookup and registry iteration statically typed while still
//! allowing the execution crate to stay generic over a provider-defined handle.

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::CoreResult;
use neo_execution::native_contract::OracleRequestDetails;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{
    ApplicationEngine, ContractState, Diagnostic, NativeContract, NativeEvent, NativeMethod,
};
use neo_payloads::{TransactionState, TrimmedBlock};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};

use super::provider::StandardNativeProvider;
use crate::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
    PolicyContract, RoleManagement, StdLib, Treasury,
};

/// Concrete handle for one of Neo N3's canonical native contracts.
#[derive(Debug, Clone, Copy)]
pub enum StandardNativeContract {
    /// ContractManagement native contract.
    ContractManagement(ContractManagement),
    /// StdLib native contract.
    StdLib(StdLib),
    /// CryptoLib native contract.
    CryptoLib(CryptoLib),
    /// LedgerContract native contract.
    LedgerContract(LedgerContract),
    /// NeoToken native contract.
    NeoToken(NeoToken),
    /// GasToken native contract.
    GasToken(GasToken),
    /// PolicyContract native contract.
    PolicyContract(PolicyContract),
    /// RoleManagement native contract.
    RoleManagement(RoleManagement),
    /// OracleContract native contract.
    OracleContract(OracleContract),
    /// Notary native contract.
    Notary(Notary),
    /// Treasury native contract.
    Treasury(Treasury),
}

macro_rules! with_standard_contract {
    ($self:expr, $contract:ident => $body:expr) => {
        match $self {
            StandardNativeContract::ContractManagement($contract) => $body,
            StandardNativeContract::StdLib($contract) => $body,
            StandardNativeContract::CryptoLib($contract) => $body,
            StandardNativeContract::LedgerContract($contract) => $body,
            StandardNativeContract::NeoToken($contract) => $body,
            StandardNativeContract::GasToken($contract) => $body,
            StandardNativeContract::PolicyContract($contract) => $body,
            StandardNativeContract::RoleManagement($contract) => $body,
            StandardNativeContract::OracleContract($contract) => $body,
            StandardNativeContract::Notary($contract) => $body,
            StandardNativeContract::Treasury($contract) => $body,
        }
    };
}

impl StandardNativeContract {
    /// Returns the canonical standard native-contract set in C# id order.
    #[must_use]
    pub fn all() -> [Self; crate::STANDARD_NATIVE_CONTRACT_COUNT] {
        [
            Self::ContractManagement(ContractManagement::new()),
            Self::StdLib(StdLib::new()),
            Self::CryptoLib(CryptoLib::new()),
            Self::LedgerContract(LedgerContract::new()),
            Self::NeoToken(NeoToken::new()),
            Self::GasToken(GasToken::new()),
            Self::PolicyContract(PolicyContract::new()),
            Self::RoleManagement(RoleManagement::new()),
            Self::OracleContract(OracleContract::new()),
            Self::Notary(Notary::new()),
            Self::Treasury(Treasury::new()),
        ]
    }

    /// Returns the canonical native contract with `hash`.
    #[must_use]
    pub fn by_hash(hash: &UInt160) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|contract| contract.hash() == *hash)
    }

    /// Returns the canonical native contract named `name`.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|contract| contract.name().eq_ignore_ascii_case(name))
    }

    /// Returns the canonical native contract id.
    #[must_use]
    pub fn id(&self) -> i32 {
        with_standard_contract!(self, contract => contract.id())
    }

    /// Returns the canonical native contract script hash.
    #[must_use]
    pub fn hash(&self) -> UInt160 {
        with_standard_contract!(self, contract => contract.hash())
    }

    /// Returns the canonical native contract name.
    #[must_use]
    pub fn name(&self) -> &str {
        with_standard_contract!(self, contract => contract.name())
    }

    /// Returns the hardfork that activates this native contract, if any.
    #[must_use]
    pub fn active_in(&self) -> Option<Hardfork> {
        with_standard_contract!(self, contract => contract.active_in())
    }

    /// Returns hardforks that refresh this native contract's stored manifest.
    #[must_use]
    pub fn activations(&self) -> &'static [Hardfork] {
        with_standard_contract!(self, contract => contract.activations())
    }

    /// Returns hardforks referenced by this native contract's metadata.
    #[must_use]
    pub fn used_hardforks(&self) -> Vec<Hardfork> {
        with_standard_contract!(self, contract => contract.used_hardforks())
    }

    /// Returns the native method metadata for this standard contract.
    #[must_use]
    pub fn methods(&self) -> &[NativeMethod] {
        with_standard_contract!(self, contract => contract.methods())
    }

    /// Returns whether this contract is active at `block_height` for the
    /// canonical standard provider.
    #[must_use]
    pub fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        <Self as NativeContract<StandardNativeProvider>>::is_active(self, settings, block_height)
    }

    /// Returns whether this height initializes or refreshes the contract.
    #[must_use]
    pub fn is_initialize_block(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> (bool, Vec<Hardfork>) {
        <Self as NativeContract<StandardNativeProvider>>::is_initialize_block(
            self,
            settings,
            block_height,
        )
    }

    /// Builds the canonical manifest-backed contract state at `block_height`.
    #[must_use]
    pub fn contract_state(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Option<ContractState> {
        <Self as NativeContract<StandardNativeProvider>>::contract_state(
            self,
            settings,
            block_height,
        )
    }

    /// Returns whether the canonical empty-block fast-forward path models all
    /// state effects of this contract.
    #[must_use]
    pub fn supports_empty_block_fast_forward(&self) -> bool {
        <Self as NativeContract<StandardNativeProvider>>::supports_empty_block_fast_forward(self)
    }
}

impl<P> NativeContract<P> for StandardNativeContract
where
    P: NativeContractProvider + 'static,
{
    fn id(&self) -> i32 {
        self.id()
    }

    fn hash(&self) -> UInt160 {
        self.hash()
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn active_in(&self) -> Option<Hardfork> {
        with_standard_contract!(self, contract => contract.active_in())
    }

    fn methods(&self) -> &[NativeMethod] {
        with_standard_contract!(self, contract => contract.methods())
    }

    fn activations(&self) -> &'static [Hardfork] {
        with_standard_contract!(self, contract => contract.activations())
    }

    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::supported_standards(contract, settings, block_height)
        })
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::event_descriptors(contract)
        })
    }

    fn invoke<D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::invoke(contract, engine, method, args)
        })
    }

    fn invoke_resolved<D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
        method_index: usize,
        method: &NativeMethod,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::invoke_resolved(contract, engine, method_index, method, args)
        })
    }

    fn initialize<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::initialize(contract, engine)
        })
    }

    fn on_persist<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::on_persist(contract, engine)
        })
    }

    fn post_persist<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::post_persist(contract, engine)
        })
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::supports_empty_block_fast_forward(contract)
        })
    }

    fn lookup_contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::lookup_contract_state(contract, snapshot, hash)
        })
    }

    fn is_contract_blocked<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::is_contract_blocked(contract, snapshot, contract_hash)
        })
    }

    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::committee_address(contract, snapshot)
        })
    }

    fn whitelisted_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::whitelisted_fee(
                contract,
                snapshot,
                contract_hash,
                method,
                param_count,
            )
        })
    }

    fn oracle_request_url_full<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::oracle_request_url_full(contract, snapshot, id)
        })
    }

    fn transaction_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<TransactionState>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::transaction_state(contract, snapshot, tx_hash)
        })
    }

    fn trimmed_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        block_hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        with_standard_contract!(self, contract => {
            <_ as NativeContract<P>>::trimmed_block(contract, snapshot, block_hash)
        })
    }
}

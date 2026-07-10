//! Native contract provider — the seam between the abstract engine and the
//! concrete native-contract implementations.
//!
//! The application engine in [`crate::ApplicationEngine`] needs to look up
//! native contracts (NEO, GAS, Policy, ContractManagement, ...) by hash, but
//! the engine itself lives in `neo-execution` while the concrete
//! implementations live in `neo-native-contracts`. To avoid the resulting
//! crate-cycle, this module exposes a `NativeContractProvider` trait that:
//!
//! - is **defined** in `neo-execution` (the consumer);
//! - is **implemented** in `neo-native-contracts` (the provider); and
//! - is provided by the composition root and captured by new application
//!   engines.
//!
//! Typical startup and engine construction:
//!
//! ```ignore
//! // In neo-system / neo-node composition:
//! let provider = Arc::new(StandardNativeContractProvider::new(settings));
//! let node = NodeBuilder::new().with_native_contract_provider(provider).build()?;
//!
//! // In tests or replay batches that need a temporary provider:
//! let engine = ApplicationEngine::new_with_native_contract_provider(
//!     trigger,
//!     container,
//!     snapshot,
//!     block,
//!     settings,
//!     gas_limit,
//!     diagnostic,
//!     Some(provider),
//! )?;
//! ```
//!
//! The trait is intentionally capability-oriented. Native invocation still
//! resolves executable contracts by hash, while reads such as policy checks,
//! ContractManagement lookups, Ledger state, Oracle request inheritance, and
//! fee settings are exposed as typed provider methods. That keeps the runtime
//! path out of process-global state and avoids resolving by native-contract
//! trait object when a concrete provider can answer directly.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_payloads::{TransactionState, TrimmedBlock};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};
use num_bigint::BigInt;

use crate::contract_state::ContractState;
use crate::native_contract::NativeContract;
use crate::native_contract::OracleRequestDetails;

/// Empty provider used by tests and engine instances that intentionally do not
/// expose native contracts.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoNativeContractProvider;

/// Uninhabited native-contract handle for providers that expose no native
/// contracts.
#[derive(Debug, Clone, Copy)]
pub enum NoNativeContract {}

/// Trait abstracting the lookup of native contracts.
pub trait NativeContractProvider: Send + Sync + Sized + 'static {
    /// Concrete native-contract handle returned by this provider.
    type Contract: NativeContract<Self> + Clone + Send + Sync + 'static;

    /// Returns the executable native contract registered under the given hash.
    ///
    /// Capability-only providers do not participate in native dispatch and can
    /// use the empty default. Standard node providers override this with the
    /// canonical native contract catalog.
    fn get_native_contract(&self, _hash: &UInt160) -> Option<Self::Contract> {
        None
    }

    /// Returns all native contracts known to this provider in the
    /// canonical registration order.
    fn all_native_contracts(&self) -> Vec<Self::Contract> {
        Vec::new()
    }

    /// Returns all native contract hashes known to this provider.
    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        Vec::new()
    }

    /// Returns LedgerContract.CurrentIndex for the supplied snapshot.
    fn current_block_index<B: CacheRead>(
        &self,
        snapshot: &neo_storage::DataCache<B>,
    ) -> neo_error::CoreResult<u32> {
        let _ = snapshot;
        Ok(0)
    }

    /// Returns StateValidator designated nodes effective at `index`.
    ///
    /// Providers that expose RoleManagement should implement this directly
    /// with their concrete contract helpers. The default is explicit so
    /// callers do not silently fall back to a type-erased lookup.
    fn state_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        let _ = (snapshot, index);
        Err(CoreError::invalid_operation(
            "native provider does not expose StateValidator designations",
        ))
    }

    /// Returns the active `PolicyContract.MaxTraceableBlocks` value.
    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        let _ = (snapshot, settings);
        Err(CoreError::invalid_operation(
            "native provider does not expose MaxTraceableBlocks",
        ))
    }

    /// Returns whether `account` is blocked by Policy.
    fn policy_is_blocked<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<bool> {
        let _ = (snapshot, account);
        Err(CoreError::invalid_operation(
            "native provider does not expose policy blocked accounts",
        ))
    }

    /// Returns a whitelisted fixed fee for a native call, if Policy defines one.
    fn policy_whitelisted_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        let _ = (snapshot, contract_hash, method, param_count);
        Err(CoreError::invalid_operation(
            "native provider does not expose policy whitelisted fees",
        ))
    }

    /// Returns the active `PolicyContract.MaxValidUntilBlockIncrement` value.
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        let _ = (snapshot, settings);
        Err(CoreError::invalid_operation(
            "native provider does not expose MaxValidUntilBlockIncrement",
        ))
    }

    /// Returns the active `PolicyContract.FeePerByte` value.
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        let _ = snapshot;
        Err(CoreError::invalid_operation(
            "native provider does not expose fee per byte",
        ))
    }

    /// Returns the active `PolicyContract.ExecFeeFactor` value.
    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        let _ = (snapshot, settings, height);
        Err(CoreError::invalid_operation(
            "native provider does not expose execution fee factor",
        ))
    }

    /// Returns the raw stored `PolicyContract.ExecFeeFactor` value.
    fn exec_fee_factor_raw<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        let _ = snapshot;
        Err(CoreError::invalid_operation(
            "native provider does not expose raw execution fee factor",
        ))
    }

    /// Returns the active `PolicyContract.StoragePrice` value.
    fn storage_price<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        let _ = snapshot;
        Err(CoreError::invalid_operation(
            "native provider does not expose storage price",
        ))
    }

    /// Returns the GAS balance of `account`.
    fn gas_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        let _ = (snapshot, account);
        Err(CoreError::invalid_operation(
            "native provider does not expose GAS balance",
        ))
    }

    /// Returns the Notary deposit balance of `account`.
    fn notary_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        let _ = (snapshot, account);
        Err(CoreError::invalid_operation(
            "native provider does not expose Notary balance",
        ))
    }

    /// Returns the Notary native contract hash.
    fn notary_hash(&self) -> CoreResult<UInt160> {
        Err(CoreError::invalid_operation(
            "native provider does not expose Notary hash",
        ))
    }

    /// Returns NEO's cached committee multisig address.
    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>> {
        let _ = snapshot;
        Err(CoreError::invalid_operation(
            "native provider does not expose committee address",
        ))
    }

    /// Returns NEO's next block validator set in C# whitelist order.
    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        let _ = (snapshot, settings);
        Err(CoreError::invalid_operation(
            "native provider does not expose next block validators",
        ))
    }

    /// Returns designated Oracle nodes effective at `height`.
    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        let _ = (snapshot, height);
        Err(CoreError::invalid_operation(
            "native provider does not expose Oracle designations",
        ))
    }

    /// Returns the Oracle native contract hash.
    fn oracle_hash(&self) -> CoreResult<UInt160> {
        Err(CoreError::invalid_operation(
            "native provider does not expose Oracle hash",
        ))
    }

    /// Returns the GAS reserved for an Oracle response request.
    fn oracle_response_gas<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<i64>> {
        let _ = (snapshot, request_id);
        Err(CoreError::invalid_operation(
            "native provider does not expose Oracle request gas",
        ))
    }

    /// Returns the deployed Oracle contract state from ContractManagement.
    fn oracle_contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<ContractState>> {
        let _ = snapshot;
        Err(CoreError::invalid_operation(
            "native provider does not expose Oracle contract state",
        ))
    }

    /// Returns a deployed contract state from ContractManagement.
    fn contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let _ = (snapshot, hash);
        Err(CoreError::invalid_operation(
            "native provider does not expose contract states",
        ))
    }

    /// Returns Oracle request details for CheckWitness signer inheritance.
    fn oracle_request_details<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        let _ = (snapshot, request_id);
        Err(CoreError::invalid_operation(
            "native provider does not expose Oracle request details",
        ))
    }

    /// Returns a persisted transaction state from Ledger.
    fn transaction_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<TransactionState>> {
        let _ = (snapshot, tx_hash);
        Err(CoreError::invalid_operation(
            "native provider does not expose transaction states",
        ))
    }

    /// Returns a trimmed block from Ledger.
    fn trimmed_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        block_hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        let _ = (snapshot, block_hash);
        Err(CoreError::invalid_operation(
            "native provider does not expose trimmed blocks",
        ))
    }

    /// Returns NEO's `NextConsensus` account for `block_index`.
    fn next_consensus_address_for_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<UInt160> {
        let _ = (snapshot, settings, block_index);
        Err(CoreError::invalid_operation(
            "native provider does not expose next consensus address",
        ))
    }

    /// Returns the active `PolicyContract.MillisecondsPerBlock` value.
    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        let _ = (snapshot, settings);
        Err(CoreError::invalid_operation(
            "native provider does not expose milliseconds per block",
        ))
    }

    /// Applies byte-equivalent NEO empty-block reward effects for `[start, end]`.
    fn fast_forward_empty_block_rewards<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        let _ = (snapshot, settings, start, end);
        Err(CoreError::invalid_operation(
            "native provider does not expose empty-block fast-forward rewards",
        ))
    }

    /// Returns the default execution fee factor used when none is
    /// configured (matches `PolicyContract.DEFAULT_EXEC_FEE_FACTOR`).
    fn default_exec_fee_factor(&self) -> u32 {
        30 // Neo N3 default
    }

    /// Returns the default storage price used when none is configured
    /// (matches `PolicyContract.DEFAULT_STORAGE_PRICE`).
    fn default_storage_price(&self) -> u32 {
        100000 // Neo N3 default
    }
}

impl<P> NativeContract<P> for NoNativeContract
where
    P: NativeContractProvider + 'static,
{
    fn id(&self) -> i32 {
        match *self {}
    }

    fn hash(&self) -> UInt160 {
        match *self {}
    }

    fn name(&self) -> &str {
        match *self {}
    }

    fn methods(&self) -> &[crate::native_contract::NativeMethod] {
        match *self {}
    }

    fn invoke<D, B>(
        &self,
        _engine: &mut crate::application_engine::ApplicationEngine<P, D, B>,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: crate::diagnostic::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        match *self {}
    }
}

impl NativeContractProvider for NoNativeContractProvider {
    type Contract = NoNativeContract;
}

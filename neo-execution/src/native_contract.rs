//! Base native contract trait and types.
//!
//! This module owns the abstract [`NativeContract`] trait and the
//! [`NativeMethod`] data struct that the application engine and the
//! concrete native contracts (in `neo-native-contracts`) both depend
//! on. The trait is defined here (with the engine, the consumer) so
//! that the engine can dispatch `System.Contract.CallNative` against
//! any `Arc<dyn NativeContract>` without depending on
//! `neo-native-contracts` directly.
//!
//! The concrete `BaseNativeContract`, the `impl_native_contract!` macro,
//! and the `build_native_contract_state` helper all live with the
//! concrete native-contract implementations in `neo-native-contracts`.

use neo_primitives::{UInt160, UInt256, ContractParameterType};
use neo_data_cache::DataCache;
use neo_block::TransactionState;

use neo_error::{CoreError as Error, CoreResult as Result};
use neo_config::{Hardfork, ProtocolSettings};
use neo_manifest::{
    ContractAbi, ContractEventDescriptor, ContractMethodDescriptor, ContractParameterDefinition,
    ContractManifest, NefFile,
};
use neo_vm_rs::OpCode;
use neo_script_builder::ScriptBuilder;
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::hardfork_activable::HardforkActivable;
use crate::application_engine::ApplicationEngine;
use crate::contract_state::ContractState;

pub use crate::native_contract_cache::{NativeContractsCache, NativeContractsCacheEntry};

/// Trait for native contract implementations.
pub trait NativeContract: Any + Send + Sync {
    /// Gets the unique identifier of the native contract.
    fn id(&self) -> i32;

    /// Gets the hash of the native contract.
    fn hash(&self) -> UInt160;

    /// Gets the name of the native contract.
    fn name(&self) -> &str;

    /// Hardfork that activates this contract (matches C# `NativeContract.ActiveIn`).
    fn active_in(&self) -> Option<Hardfork> {
        None
    }

    /// Gets the supported methods of the native contract.
    fn methods(&self) -> &[NativeMethod];

    /// Determines whether the native contract is active under the given settings.
    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        match self.active_in() {
            // C# NativeContract.IsActive checks IsHardforkEnabled(ActiveIn, height),
            // which returns false when the hardfork is NOT configured. An unscheduled
            // ActiveIn hardfork therefore means the contract is never active. Treating
            // a missing activation height as 0 (the old `unwrap_or(0)`) would wrongly
            // activate/deploy the contract from genesis and diverge the state root.
            Some(hardfork) => settings.is_hardfork_enabled(hardfork, block_height),
            None => true,
        }
    }

    /// Returns hardforks that should trigger a contract manifest refresh.
    ///
    /// This mirrors C# `NativeContract.Activations` for manifest-only updates
    /// such as supported standards changes.
    fn activations(&self) -> Vec<Hardfork> {
        Vec::new()
    }

    /// Returns the hardforks that affect this contract (methods/activations).
    ///
    /// Mirrors C# NativeContract `_usedHardforks` (excluding event attributes).
    fn used_hardforks(&self) -> Vec<Hardfork> {
        let mut hardforks = Vec::new();

        if let Some(hardfork) = self.active_in() {
            hardforks.push(hardfork);
        }

        for method in self.methods() {
            if let Some(hardfork) = method.active_in {
                hardforks.push(hardfork);
            }
            if let Some(hardfork) = method.deprecated_in {
                hardforks.push(hardfork);
            }
        }

        hardforks.extend(self.activations());
        hardforks.sort();
        hardforks.dedup();
        hardforks
    }

    /// Returns whether this block should initialize/refresh the native contract.
    ///
    /// Matches C# `NativeContract.IsInitializeBlock` semantics.
    fn is_initialize_block(
        &self,
        settings: &ProtocolSettings,
        index: u32,
    ) -> (bool, Vec<Hardfork>) {
        let mut hits = Vec::new();
        for hardfork in self.used_hardforks() {
            if let Some(&activation_height) = settings.hardforks.get(&hardfork) {
                if activation_height == index {
                    hits.push(hardfork);
                }
            }
        }

        if !hits.is_empty() {
            return (true, hits);
        }

        if index == 0 && self.active_in().is_none() {
            return (true, Vec::new());
        }

        (false, Vec::new())
    }

    /// Returns the contract state metadata if available.
    fn contract_state(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Option<ContractState> {
        if !self.is_active(settings, block_height) {
            return None;
        }
        Some(build_native_contract_state(self, settings, block_height))
    }

    /// Returns supported standards for the contract manifest.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        Vec::new()
    }

    /// Returns event descriptors for the contract manifest ABI.
    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        Vec::new()
    }

    /// Invokes a method on the native contract.
    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>>;

    /// Called when the contract is initialized.
    fn initialize(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        Ok(())
    }

    /// Called on each block persistence.
    fn on_persist(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        Ok(())
    }

    /// Called after block persistence.
    fn post_persist(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        Ok(())
    }

    /// Returns the contract state for a deployed contract by hash, if
    /// this contract stores contract-state records.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `ContractManagement` overrides this to query the storage.
    fn lookup_contract_state(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _hash: &neo_primitives::UInt160,
    ) -> Result<Option<crate::ContractState>> {
        Ok(None)
    }

    /// Returns the transaction state for a transaction by hash, if
    /// this contract stores transaction records.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `LedgerContract` overrides this.
    fn lookup_transaction_state(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _tx_hash: &neo_primitives::UInt256,
    ) -> Result<Option<neo_block::TransactionState>> {
        Ok(None)
    }

    /// Returns whether the given contract hash is currently blocked
    /// from being invoked.
    ///
    /// The default implementation returns `Ok(false)`; only
    /// `PolicyContract` overrides this.
    fn is_contract_blocked(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _contract_hash: &neo_primitives::UInt160,
    ) -> Result<bool> {
        Ok(false)
    }

    /// Returns the committee multisig address (C# `NEO.GetCommitteeAddress`),
    /// used by `check_committee_witness` to authorize committee-gated operations.
    ///
    /// The default implementation returns `Ok(None)`; only `NeoToken` (which
    /// owns the committee cache) overrides this.
    fn committee_address(
        &self,
        _snapshot: &neo_data_cache::DataCache,
    ) -> Result<Option<neo_primitives::UInt160>> {
        Ok(None)
    }

    /// Returns the whitelisted fee (in datoshi) for the given contract
    /// method, or `None` if no whitelist applies.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `PolicyContract` overrides this.
    fn whitelisted_fee(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _contract_hash: &neo_primitives::UInt160,
        _method: &str,
        _param_count: u32,
    ) -> Result<Option<i64>> {
        Ok(None)
    }

    /// Returns the oracle request for a given request ID, if any.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `OracleContract` overrides this.
    fn oracle_request_url(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _id: u64,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    fn oracle_request_original_tx(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _id: u64,
    ) -> Result<Option<neo_primitives::UInt256>> {
        Ok(None)
    }

    /// Returns the URL of the oracle request, if any.
    fn oracle_request_url_full(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _id: u64,
    ) -> Result<Option<OracleRequestDetails>> {
        Ok(None)
    }

    /// Returns the transaction state for a given transaction hash.
    fn transaction_state(
        &self,
        _snapshot: &neo_data_cache::DataCache,
        _tx_hash: &neo_primitives::UInt256,
    ) -> Result<Option<neo_block::TransactionState>> {
        Ok(None)
    }

    /// Returns the default execution fee factor used when no
    /// policy-set value is configured.
    fn default_exec_fee_factor(&self) -> u32 {
        30
    }

    /// Returns the default storage price used when no policy-set
    /// value is configured.
    fn default_storage_price(&self) -> u32 {
        100_000
    }

    /// Returns a reference to self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Lightweight oracle request descriptor used by the engine.
#[derive(Clone, Debug)]
pub struct OracleRequestDetails {
    /// The URL the oracle was asked to fetch.
    pub url: String,
    /// The original transaction id that submitted the request.
    pub original_tx_id: neo_primitives::UInt256,
}

impl OracleRequestDetails {
    /// Creates a new request detail.
    pub fn new(url: impl Into<String>, original_tx_id: neo_primitives::UInt256) -> Self {
        Self { url: url.into(), original_tx_id }
    }
}

/// Represents a method in a native contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeMethod {
    /// The name of the method.
    pub name: String,

    /// The CPU fee for the method (in execution units, multiplied by `ExecFeeFactor`).
    #[serde(alias = "gas_cost")]
    pub cpu_fee: i64,

    /// The storage fee for the method (in storage units, multiplied by `StoragePrice`).
    #[serde(default)]
    pub storage_fee: i64,

    /// Whether the method is safe (read-only).
    pub safe: bool,

    /// The required call flags for this method.
    pub required_call_flags: u8,

    /// Parameter types for the method (used for native manifest ABI).
    pub parameters: Vec<ContractParameterType>,

    /// Parameter names for the method (used for native manifest ABI).
    ///
    /// When omitted or when the length does not match `parameters`, the ABI
    /// builder falls back to `arg{index}` names.
    #[serde(default)]
    pub parameter_names: Vec<String>,

    /// Hardfork that activates this method (matches C# `ContractMethodAttribute.ActiveIn`).
    #[serde(default)]
    pub active_in: Option<Hardfork>,

    /// Hardfork that deprecates this method (matches C# `ContractMethodAttribute.DeprecatedIn`).
    #[serde(default)]
    pub deprecated_in: Option<Hardfork>,

    /// Return type for the method (used for native manifest ABI).
    pub return_type: ContractParameterType,
}

impl NativeMethod {
    /// Creates a new native method.
    pub fn new(
        name: String,
        cpu_fee: i64,
        safe: bool,
        required_call_flags: u8,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self {
            name,
            cpu_fee,
            storage_fee: 0,
            safe,
            required_call_flags,
            parameters,
            parameter_names: Vec::new(),
            active_in: None,
            deprecated_in: None,
            return_type,
        }
    }

    /// Marks the method as active starting from the given hardfork (inclusive).
    pub fn with_active_in(mut self, hardfork: Hardfork) -> Self {
        self.active_in = Some(hardfork);
        self
    }

    /// Marks the method as deprecated starting from the given hardfork (inclusive).
    pub fn with_deprecated_in(mut self, hardfork: Hardfork) -> Self {
        self.deprecated_in = Some(hardfork);
        self
    }

    /// Overrides the required call flags for this method.
    pub fn with_required_call_flags(
        mut self,
        flags: neo_manifest::CallFlags,
    ) -> Self {
        self.required_call_flags = flags.bits();
        self
    }

    /// Sets parameter names used in the generated native manifest ABI.
    pub fn with_parameter_names(mut self, parameter_names: Vec<String>) -> Self {
        self.parameter_names = parameter_names;
        self
    }

    /// Sets the storage fee for this method (multiplied by `StoragePrice`).
    pub fn with_storage_fee(mut self, storage_fee: i64) -> Self {
        self.storage_fee = storage_fee;
        self
    }

    /// Returns true if this method is active at the given height.
    pub fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        is_active_for(
            self,
            |hardfork, height| settings.is_hardfork_enabled(hardfork, height),
            block_height,
        )
    }

    /// Creates a new safe (read-only) method.
    pub fn safe(
        name: String,
        gas_cost: i64,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self::new(name, gas_cost, true, 0, parameters, return_type)
    }

    /// Creates a new unsafe (state-changing) method.
    pub fn unsafe_method(
        name: String,
        gas_cost: i64,
        required_call_flags: u8,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self::new(
            name,
            gas_cost,
            false,
            required_call_flags,
            parameters,
            return_type,
        )
    }
}

impl HardforkActivable for NativeMethod {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

/// Checks whether a hardfork-activable item is active.
///
/// Mirrors C# `NativeContract.IsActive(...)` hardfork activation semantics.
pub fn is_active_for<T: HardforkActivable>(
    item: &T,
    hf_checker: impl Fn(Hardfork, u32) -> bool,
    block_height: u32,
) -> bool {
    (item.active_in().is_none() && item.deprecated_in().is_none())
        || (item.deprecated_in().is_some()
            && !hf_checker(item.deprecated_in().unwrap(), block_height))
        || (item.active_in().is_some() && hf_checker(item.active_in().unwrap(), block_height))
}

/// Builds a [`ContractState`] for a native contract at the given
/// block height.
///
/// Mirrors the C# logic that produces the on-disk contract state for
/// each native contract, including the NEF bytecode that the
/// application engine loads when dispatching `System.Contract.CallNative`.
pub fn build_native_contract_state<T: NativeContract + ?Sized>(
    contract: &T,
    settings: &ProtocolSettings,
    block_height: u32,
) -> ContractState {
    use neo_manifest::{
        ContractAbi, ContractEventDescriptor, ContractMethodDescriptor, ContractParameterDefinition,
        ContractManifest, NefFile,
    };
    use neo_script_builder::ScriptBuilder;
    use neo_vm_rs::OpCode;

    let syscall_hash = neo_vm_rs::interop_hash("System.Contract.CallNative");

    let mut methods: Vec<&NativeMethod> = contract
        .methods()
        .iter()
        .filter(|method| method.is_active(settings, block_height))
        .collect();
    methods.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.parameters.len().cmp(&b.parameters.len()))
    });

    let mut builder = ScriptBuilder::new();
    let mut abi_methods = Vec::with_capacity(methods.len());

    for method in methods {
        let offset = builder.len() as i32;
        builder.emit_push_int(0);
        builder.emit_syscall_hash(syscall_hash);
        builder.emit_opcode(OpCode::RET);

        let has_names = method.parameter_names.len() == method.parameters.len();
        let parameters = method
            .parameters
            .iter()
            .enumerate()
            .map(|(index, param_type)| {
                let name = if has_names {
                    method.parameter_names[index].clone()
                } else {
                    format!("arg{index}")
                };
                ContractParameterDefinition::new(name, *param_type)
                    .expect("native parameter definition")
            })
            .collect();

        let descriptor = ContractMethodDescriptor::new(
            method.name.clone(),
            parameters,
            method.return_type,
            offset,
            method.safe,
        )
        .expect("native method descriptor");
        abi_methods.push(descriptor);
    }

    let nef = NefFile::new("neo-core-v3.0".to_string(), builder.to_array());

    let mut manifest = ContractManifest::new_native(contract.name().to_string());
    manifest.supported_standards = contract.supported_standards(settings, block_height);
    let events: Vec<ContractEventDescriptor> = contract.events(settings, block_height);
    manifest.abi = ContractAbi::new(abi_methods, events);

    ContractState::new(contract.id(), contract.hash(), nef, manifest)
}

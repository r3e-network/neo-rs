//! Base native contract trait and types.

use crate::UInt160;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractMethodDescriptor, ContractParameterDefinition,
};
use crate::smart_contract::native::IHardforkActivable;
use crate::smart_contract::{ContractManifest, ContractParameterType, ContractState, NefFile};
use neo_vm::{OpCode, ScriptBuilder};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;

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
            Some(hardfork) => {
                let activation_height = settings.hardforks.get(&hardfork).copied().unwrap_or(0);
                block_height >= activation_height
            }
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

    /// Returns a reference to self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
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
        flags: crate::smart_contract::call_flags::CallFlags,
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

impl IHardforkActivable for NativeMethod {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

/// Checks whether a hardfork-activable item is active.
///
/// Mirrors C# `NativeContract.IsActive(IHardforkActivable, ...)` exactly.
pub fn is_active_for<T: IHardforkActivable>(
    item: &T,
    hf_checker: impl Fn(Hardfork, u32) -> bool,
    block_height: u32,
) -> bool {
    (item.active_in().is_none() && item.deprecated_in().is_none())
        || (item.deprecated_in().is_some()
            && !hf_checker(item.deprecated_in().unwrap(), block_height))
        || (item.active_in().is_some() && hf_checker(item.active_in().unwrap(), block_height))
}

/// Base implementation for native contracts.
pub struct BaseNativeContract {
    /// The hash of the contract.
    pub hash: UInt160,

    /// The name of the contract.
    pub name: String,

    /// The supported methods.
    pub methods: Vec<NativeMethod>,
}

impl BaseNativeContract {
    /// Creates a new base native contract.
    pub fn new(hash: UInt160, name: String, methods: Vec<NativeMethod>) -> Self {
        Self {
            hash,
            name,
            methods,
        }
    }

    /// Finds a method by name.
    pub fn find_method(&self, name: &str) -> Option<&NativeMethod> {
        self.methods.iter().find(|m| m.name == name)
    }

    /// Validates that a method exists and can be called.
    pub fn validate_method_call(&self, method: &str) -> Result<&NativeMethod> {
        self.find_method(method)
            .ok_or_else(|| Error::native_contract(format!("Method not found: {}", method)))
    }
}

/// Macro to help implement native contracts.
#[macro_export]
macro_rules! impl_native_contract {
    ($contract:ty, $hash:expr_2021, $name:expr_2021, $methods:expr_2021) => {
        impl NativeContract for $contract {
            fn hash(&self) -> UInt160 {
                $hash
            }

            fn name(&self) -> &str {
                $name
            }

            fn methods(&self) -> &[NativeMethod] {
                &$methods
            }

            fn invoke(
                &self,
                engine: &mut ApplicationEngine,
                method: &str,
                args: &[Vec<u8>],
            ) -> Result<Vec<u8>> {
                self.invoke_method(engine, method, args)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

/// Cache of native contract method metadata, mirroring the C# NativeContractsCache behaviour.
#[derive(Default)]
pub struct NativeContractsCache {
    entries: HashMap<i32, NativeContractsCacheEntry>,
}

impl NativeContractsCache {
    /// Gets the cached entry for the given native contract, building it on demand.
    pub fn get_or_build<'a>(
        &'a mut self,
        contract: &dyn NativeContract,
    ) -> &'a NativeContractsCacheEntry {
        let contract_id = contract.id();
        self.entries
            .entry(contract_id)
            .or_insert_with(|| NativeContractsCacheEntry::from_contract(contract))
    }
}

/// Cached metadata for a single native contract.
pub struct NativeContractsCacheEntry {
    methods_by_name: HashMap<String, Vec<NativeMethod>>,
}

impl NativeContractsCacheEntry {
    fn from_contract(contract: &dyn NativeContract) -> Self {
        let mut methods_by_name: HashMap<String, Vec<NativeMethod>> = HashMap::new();
        for method in contract.methods() {
            methods_by_name
                .entry(method.name.clone())
                .or_default()
                .push(method.clone());
        }

        Self { methods_by_name }
    }

    /// Gets the method metadata entry matching `name`, `parameter_count`, and activation state.
    pub fn get_method(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Result<Option<&NativeMethod>> {
        let Some(candidates) = self.methods_by_name.get(name) else {
            return Ok(None);
        };

        let mut active = candidates.iter().filter(|method| {
            method.parameters.len() == parameter_count && method.is_active(settings, block_height)
        });

        let Some(selected) = active.next() else {
            return Ok(None);
        };

        if active.next().is_some() {
            return Err(Error::invalid_operation(format!(
                "Ambiguous native method '{}({})' at height {}",
                name, parameter_count, block_height
            )));
        }

        Ok(Some(selected))
    }
}

fn build_native_contract_state<T: NativeContract + ?Sized>(
    contract: &T,
    settings: &ProtocolSettings,
    block_height: u32,
) -> ContractState {
    let syscall_hash = ScriptBuilder::hash_syscall("System.Contract.CallNative")
        .expect("System.Contract.CallNative syscall hash must be computable");

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
    manifest.abi = ContractAbi::new(abi_methods, contract.events(settings, block_height));

    ContractState::new(contract.id(), contract.hash(), nef, manifest)
}

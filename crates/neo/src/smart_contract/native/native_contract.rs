//! Base native contract trait and types.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::manifest::ContractMethodDescriptor;
use crate::UInt160;
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

    /// Gets the supported methods of the native contract.
    fn methods(&self) -> &[NativeMethod];

    /// Determines whether the native contract is active under the given settings.
    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    /// Returns the contract state metadata if available.
    fn contract_state(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Option<crate::smart_contract::ContractState> {
        None
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

    /// The gas cost of the method.
    pub gas_cost: i64,

    /// Whether the method is safe (read-only).
    pub safe: bool,

    /// The required call flags for this method.
    pub required_call_flags: u8,
}

impl NativeMethod {
    /// Creates a new native method.
    pub fn new(name: String, gas_cost: i64, safe: bool, required_call_flags: u8) -> Self {
        Self {
            name,
            gas_cost,
            safe,
            required_call_flags,
        }
    }

    /// Creates a new safe (read-only) method.
    pub fn safe(name: String, gas_cost: i64) -> Self {
        Self::new(name, gas_cost, true, 0)
    }

    /// Creates a new unsafe (state-changing) method.
    pub fn unsafe_method(name: String, gas_cost: i64, required_call_flags: u8) -> Self {
        Self::new(name, gas_cost, false, required_call_flags)
    }
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
            .ok_or_else(|| Error::NativeContractError(format!("Method not found: {}", method)))
    }
}

/// Macro to help implement native contracts.
#[macro_export]
macro_rules! impl_native_contract {
    ($contract:ty, $hash:expr, $name:expr, $methods:expr) => {
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
    methods_by_name: HashMap<String, NativeMethod>,
}

impl NativeContractsCacheEntry {
    fn from_contract(contract: &dyn NativeContract) -> Self {
        let methods_by_name = contract
            .methods()
            .iter()
            .map(|method| (method.name.clone(), method.clone()))
            .collect();

        Self { methods_by_name }
    }

    /// Gets a method metadata entry by name.
    pub fn get_method(&self, name: &str) -> Option<&NativeMethod> {
        self.methods_by_name.get(name)
    }
}

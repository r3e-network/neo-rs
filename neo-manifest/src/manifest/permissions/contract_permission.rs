//!
//! Defines the permissions that a contract requires to call other contracts
//! and access their methods.

use super::{ContractManifest, ContractPermissionDescriptor, WildCardContainer};
use neo_crypto::ECPoint;
use neo_error::CoreError;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
// Removed neo_cryptography dependency - using external crypto crates directly
use neo_vm::StackItem;
use serde::{Deserialize, Serialize};

/// Represents a permission that a contract requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractPermission {
    /// The contract or group that this permission applies to.
    pub contract: ContractPermissionDescriptor,

    /// The methods that are allowed to be called.
    pub methods: WildCardContainer<String>,
}

impl ContractPermission {
    /// Creates a new contract permission.
    pub fn new(contract: ContractPermissionDescriptor, methods: WildCardContainer<String>) -> Self {
        Self { contract, methods }
    }

    /// Creates a wildcard permission that allows calling any method on any contract.
    pub fn default_wildcard() -> Self {
        Self {
            contract: ContractPermissionDescriptor::create_wildcard(),
            methods: WildCardContainer::create_wildcard(),
        }
    }

    /// Creates a permission for a specific contract hash.
    pub fn for_contract(hash: UInt160, methods: WildCardContainer<String>) -> Self {
        Self {
            contract: ContractPermissionDescriptor::Hash(hash),
            methods,
        }
    }

    /// Creates a permission for a specific group public key.
    pub fn for_group(public_key: ECPoint, methods: WildCardContainer<String>) -> Self {
        Self {
            contract: ContractPermissionDescriptor::Group(public_key),
            methods,
        }
    }

    /// Creates from JSON.
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;
        let contract = ContractPermissionDescriptor::from_json(
            obj.get("contract")
                .ok_or_else(|| CoreError::other("Missing contract"))?,
        )?;
        let methods = WildCardContainer::<String>::from_json(
            obj.get("methods")
                .ok_or_else(|| CoreError::other("Missing methods"))?,
        )?;

        if let WildCardContainer::List(methods_list) = &methods {
            if methods_list.iter().any(String::is_empty) {
                return Err(CoreError::other(
                    "Methods in ContractPermission has empty string",
                ));
            }
            let mut seen = std::collections::HashSet::new();
            for method in methods_list {
                if !seen.insert(method) {
                    return Err(CoreError::other(
                        "Methods in ContractPermission must be unique",
                    ));
                }
            }
        }

        Ok(Self { contract, methods })
    }

    /// Checks if this permission allows interacting with the supplied contract hash and method.
    pub fn is_allowed(
        &self,
        manifest: &ContractManifest,
        contract_hash: &UInt160,
        method: &str,
    ) -> bool {
        self.allows_contract(contract_hash, manifest) && self.allows_method(method)
    }

    /// Checks if this permission allows calling a specific contract.
    pub fn allows_contract(&self, contract_hash: &UInt160, manifest: &ContractManifest) -> bool {
        self.contract
            .matches_contract(contract_hash, &manifest.groups)
    }

    /// Checks if this permission allows calling a specific method.
    pub fn allows_method(&self, method_name: &str) -> bool {
        match &self.methods {
            WildCardContainer::Wildcard => true,
            WildCardContainer::List(methods) => methods.iter().any(|m| m == method_name),
        }
    }

    /// Gets the size of the permission in bytes.
    pub fn size(&self) -> usize {
        let contract_size = self.contract.size();
        let methods_size = match &self.methods {
            WildCardContainer::Wildcard => 1,
            WildCardContainer::List(methods) => methods.iter().map(|m| m.len()).sum(),
        };
        contract_size + methods_size
    }

    /// Validates the permission.
    pub fn validate(&self) -> CoreResult<()> {
        match &self.contract {
            ContractPermissionDescriptor::Wildcard | ContractPermissionDescriptor::Hash(_) => {}
            ContractPermissionDescriptor::Group(pubkey) => {
                if !pubkey.is_valid() {
                    return Err(CoreError::invalid_data(
                        "Invalid public key in contract permission",
                    ));
                }
            }
        }

        match &self.methods {
            WildCardContainer::Wildcard => {}
            WildCardContainer::List(methods) => {
                if methods.iter().any(|m| m.is_empty()) {
                    return Err(CoreError::invalid_data("Method name cannot be empty"));
                }
            }
        }

        Ok(())
    }

    /// Converts to a neo-vm stack item (matches C# `ContractPermission.ToStackItem` layout).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            self.contract.to_stack_item(),
            self.methods.to_stack_item(),
        ])
    }

    /// Updates this permission from a neo-vm stack item.
    pub fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let StackItem::Struct(structure) = stack_item else {
            return Err(CoreError::invalid_format(
                "ContractPermission expects Struct stack item",
            ));
        };
        let items = structure.items();

        if items.len() < 2 {
            return Err(CoreError::invalid_format(format!(
                "ContractPermission stack item must contain 2 elements, found {}",
                items.len()
            )));
        }

        self.contract = ContractPermissionDescriptor::from_stack_item(&items[0]).map_err(|e| {
            CoreError::invalid_format(format!("Invalid contract descriptor in stack item: {}", e))
        })?;

        self.methods = WildCardContainer::from_stack_item(&items[1]).map_err(|e| {
            CoreError::invalid_format(format!("Invalid methods container in stack item: {}", e))
        })?;

        Ok(())
    }
}

impl Interoperable for ContractPermission {
    fn from_stack_item(&mut self, value: StackItem) -> Result<(), InteroperableError> {
        self.from_stack_item(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(self.to_stack_item())
    }
}

#[cfg(test)]
#[path = "../../tests/manifest/contract_permission.rs"]
mod tests;

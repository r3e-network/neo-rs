//!
//! Defines the permissions that a contract requires to call other contracts
//! and access their methods.

use super::{ContractManifest, ContractPermissionDescriptor, WildCardContainer};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::ECPoint;
use crate::UInt160;
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
    pub fn validate(&self) -> Result<()> {
        match &self.contract {
            ContractPermissionDescriptor::Wildcard | ContractPermissionDescriptor::Hash(_) => {}
            ContractPermissionDescriptor::Group(pubkey) => {
                if !pubkey.is_valid() {
                    return Err(Error::invalid_data(
                        "Invalid public key in contract permission",
                    ));
                }
            }
        }

        match &self.methods {
            WildCardContainer::Wildcard => {}
            WildCardContainer::List(methods) => {
                if methods.is_empty() {
                    return Err(Error::invalid_data("Method list cannot be empty"));
                }

                if methods.iter().any(|m| m.is_empty()) {
                    return Err(Error::invalid_data("Method name cannot be empty"));
                }
            }
        }

        Ok(())
    }
}

impl IInteroperable for ContractPermission {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        let struct_item = match stack_item {
            StackItem::Struct(struct_item) => struct_item,
            other => {
                tracing::error!(
                    "ContractPermission expects struct stack item, found {:?}",
                    other.stack_item_type()
                );
                return;
            }
        };

        let items = struct_item.items();
        if items.len() < 2 {
            tracing::error!("ContractPermission stack item must contain two elements");
            return;
        }

        match ContractPermissionDescriptor::from_stack_item(&items[0]) {
            Ok(contract) => self.contract = contract,
            Err(e) => {
                tracing::error!("Invalid contract descriptor in stack item: {}", e);
                return;
            }
        }

        match WildCardContainer::from_stack_item(&items[1]) {
            Ok(methods) => self.methods = methods,
            Err(e) => {
                tracing::error!("Invalid methods container in stack item: {}", e);
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            self.contract.to_stack_item(),
            self.methods.to_stack_item(),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

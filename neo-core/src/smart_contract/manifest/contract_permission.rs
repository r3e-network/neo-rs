//!
//! Defines the permissions that a contract requires to call other contracts
//! and access their methods.

use super::{ContractManifest, ContractPermissionDescriptor, WildCardContainer};
use crate::error::CoreError;
use crate::error::CoreError as Error;
use crate::error::CoreResult;
use crate::smart_contract::interoperable::IInteroperable;
use crate::vm_runtime::StackItem;
use crate::ECPoint;
use crate::UInt160;
// Removed neo_cryptography dependency - using external crypto crates directly
use neo_vm_rs::StackValue;
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
    pub fn validate(&self) -> CoreResult<()> {
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

    /// Converts to a neo-vm-rs stack value (matches C# `ContractPermission.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            self.contract.to_stack_value(),
            self.methods.to_stack_value(),
        ])
    }

    /// Updates this permission from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let StackValue::Struct(items) = stack_value else {
            return Err(CoreError::invalid_format(
                "ContractPermission expects Struct stack value",
            ));
        };

        if items.len() < 2 {
            return Err(CoreError::invalid_format(format!(
                "ContractPermission stack value must contain 2 elements, found {}",
                items.len()
            )));
        }

        self.contract =
            ContractPermissionDescriptor::from_stack_value(items[0].clone()).map_err(|e| {
                CoreError::invalid_format(format!(
                    "Invalid contract descriptor in stack value: {}",
                    e
                ))
            })?;

        self.methods = WildCardContainer::from_stack_value(items[1].clone()).map_err(|e| {
            CoreError::invalid_format(format!("Invalid methods container in stack value: {}", e))
        })?;

        Ok(())
    }
}

impl IInteroperable for ContractPermission {
    fn from_stack_item(&mut self, stack_item: StackItem) -> std::result::Result<(), CoreError> {
        self.from_stack_value(StackValue::try_from(stack_item).map_err(|error| {
            CoreError::invalid_format(format!(
                "Failed to convert ContractPermission StackItem to StackValue: {error}"
            ))
        })?)
    }

    fn to_stack_item(&self) -> std::result::Result<StackItem, CoreError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            CoreError::invalid_operation(format!(
                "Failed to convert ContractPermission StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_contract::interoperable::IInteroperable;
    use neo_vm_rs::StackValue;

    #[test]
    fn contract_permission_projects_to_neo_vm_rs_stack_value() {
        let hash = UInt160::from_bytes(&[0x44; 20]).expect("hash");
        let permission = ContractPermission::for_contract(
            hash,
            WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]),
        );

        assert_eq!(
            permission.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::ByteString(hash.to_bytes()),
                StackValue::Array(vec![
                    StackValue::ByteString(b"transfer".to_vec()),
                    StackValue::ByteString(b"balanceOf".to_vec()),
                ]),
            ])
        );
    }

    #[test]
    fn contract_permission_stack_item_projection_matches_stack_value_projection() {
        let permission = ContractPermission::default_wildcard();
        let expected = StackItem::try_from(permission.to_stack_value()).unwrap();

        assert_eq!(permission.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn contract_permission_reads_from_neo_vm_rs_stack_value() {
        let hash = UInt160::from_bytes(&[0x55; 20]).expect("hash");
        let mut permission = ContractPermission::default_wildcard();

        permission
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(hash.to_bytes()),
                StackValue::Array(vec![StackValue::ByteString(b"mint".to_vec())]),
            ]))
            .unwrap();

        assert_eq!(
            permission.contract,
            ContractPermissionDescriptor::Hash(hash)
        );
        assert_eq!(
            permission.methods,
            WildCardContainer::create(vec!["mint".to_string()])
        );
    }
}

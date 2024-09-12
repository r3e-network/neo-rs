use neo::prelude::*;
use neo::io::*;
use neo::vm::*;
use neo::types::*;
use neo::json::Json;
use std::collections::HashSet;
use crate::neo_contract::manifest::contract_permission_descriptor::ContractPermissionDescriptor;
use crate::neo_contract::manifest::wild_card_container::WildcardContainer;

/// Represents a permission of a contract. It describes which contracts may be
/// invoked and which methods are called.
/// If a contract invokes a contract or method that is not declared in the manifest
/// at runtime, the invocation will fail.
#[derive(Clone, Debug)]
pub struct ContractPermission {
    /// Indicates which contract to be invoked.
    /// It can be a hash of a contract, a public key of a group, or a wildcard *.
    /// If it specifies a hash of a contract, then the contract will be invoked;
    /// If it specifies a public key of a group, then any contract in this group
    /// may be invoked; If it specifies a wildcard *, then any contract may be invoked.
    pub contract: ContractPermissionDescriptor,

    /// Indicates which methods to be called.
    /// It can also be assigned with a wildcard *. If it is a wildcard *,
    /// then it means that any method can be called.
    pub methods: WildcardContainer<String>,
}

impl ContractPermission {
    /// A default permission that both `contract` and `methods` fields are set to wildcard *.
    pub const DEFAULT_PERMISSION: Self = Self {
        contract: ContractPermissionDescriptor::Wildcard,
        methods: WildcardContainer::Wildcard,
    };

    /// Converts the permission from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The permission represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted permission.
    pub fn from_json(json: &Json) -> Result<Self, Error> {
        let contract = ContractPermissionDescriptor::from_json(&json["contract"])?;
        let methods = WildcardContainer::from_json(&json["methods"], |u| u.as_str().map(String::from))?;

        if methods.iter().any(|p| p.is_empty()) {
            return Err(Error::Format);
        }

        let _ = methods.iter().collect::<HashSet<_>>();

        Ok(Self { contract, methods })
    }

    /// Converts the permission to a JSON object.
    ///
    /// # Returns
    ///
    /// The permission represented by a JSON object.
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("contract", self.contract.to_json());
        json.insert("methods", self.methods.to_json(|p| p.clone().into()));
        json
    }

    /// Determines whether the method of the specified contract can be called by this contract.
    ///
    /// # Arguments
    ///
    /// * `target_contract` - The contract being called.
    /// * `target_method` - The method of the specified contract.
    ///
    /// # Returns
    ///
    /// `true` if the contract allows to be called; otherwise, `false`.
    pub fn is_allowed(&self, target_contract: &ContractState, target_method: &str) -> bool {
        match &self.contract {
            ContractPermissionDescriptor::Hash(hash) => {
                if hash != &target_contract.hash {
                    return false;
                }
            }
            ContractPermissionDescriptor::Group(group) => {
                if !target_contract.manifest.groups.iter().any(|p| &p.pubkey == group) {
                    return false;
                }
            }
            ContractPermissionDescriptor::Wildcard => {}
        }
        self.methods.is_wildcard() || self.methods.contains(target_method)
    }
}

impl IInteroperable for ContractPermission {
    type Error = std::io::Error;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            let contract = match &s[0] {
                StackItem::Null => ContractPermissionDescriptor::Wildcard,
                item => ContractPermissionDescriptor::from_stack_item(item)?,
            };
            let methods = match &s[1] {
                StackItem::Null => WildcardContainer::Wildcard,
                StackItem::Array(array) => WildcardContainer::Create(
                    array.iter().map(|p| p.as_string()).collect::<Result<Vec<_>, _>>()?,
                ),
                _ => return Err(Error::InvalidFormat),
            };
            Ok(Self { contract, methods })
        } else {
            Err(Error::InvalidFormat)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Struct(Struct::new(vec![
            match &self.contract {
                ContractPermissionDescriptor::Wildcard => StackItem::Null,
                ContractPermissionDescriptor::Hash(hash) => StackItem::ByteString(hash.to_vec()),
                ContractPermissionDescriptor::Group(group) => StackItem::ByteString(group.to_vec()),
            },
            match &self.methods {
                WildcardContainer::Wildcard => StackItem::Null,
                WildcardContainer::Create(methods) => StackItem::Array(
                    methods.iter().map(|m| StackItem::String(m.clone())).collect(),
                ),
            },
            ])))
    }
}

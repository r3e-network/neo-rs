#![allow(clippy::mutable_key_type)]

//! Contract manifest implementation.
//!
//! Represents the manifest of a smart contract which declares the features
//! and permissions it will use when deployed.

use crate::manifest::{
    ContractAbi, ContractGroup, ContractPermission, ContractPermissionDescriptor, WildCardContainer,
};
use neo_error::CoreError;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

/// Maximum length of a contract manifest in bytes.
pub const MAX_MANIFEST_LENGTH: usize = u16::MAX as usize;

mod fields;
mod json;
mod stack;
mod validation;
mod wire;

pub use fields::{ManifestExtra, ManifestFeatures};

/// Represents the manifest of a smart contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractManifest {
    /// The name of the contract.
    pub name: String,

    /// The groups that the contract belongs to.
    #[serde(default)]
    pub groups: Vec<ContractGroup>,

    /// The features supported by the contract.
    #[serde(default)]
    pub features: ManifestFeatures,

    /// The standards supported by the contract.
    #[serde(default, rename = "supportedstandards")]
    pub supported_standards: Vec<String>,

    /// The ABI (Application Binary Interface) of the contract.
    pub abi: ContractAbi,

    /// The permissions required by the contract.
    #[serde(default)]
    pub permissions: Vec<ContractPermission>,

    /// The contracts and groups that this contract trusts.
    #[serde(default)]
    pub trusts: WildCardContainer<ContractPermissionDescriptor>,

    /// Additional metadata.
    #[serde(default)]
    pub extra: Option<ManifestExtra>,
}

impl ContractManifest {
    /// Creates a new contract manifest.
    pub fn new(name: String) -> Self {
        Self {
            name,
            groups: Vec::new(),
            features: ManifestFeatures::empty(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::create_wildcard(),
            extra: None,
        }
    }

    /// Creates a new native contract manifest.
    pub fn new_native(name: String) -> Self {
        Self {
            name,
            groups: Vec::new(),
            features: ManifestFeatures::empty(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::default(),
            extra: None,
        }
    }

    /// Converts the manifest to JSON.
    pub fn to_json(&self) -> CoreResult<Value> {
        serde_json::to_value(self).map_err(|e| CoreError::serialization(e.to_string()))
    }

    /// Checks if the contract can call another contract.
    pub fn can_call(
        &self,
        target_manifest: &ContractManifest,
        target_hash: &UInt160,
        target_method: &str,
    ) -> bool {
        match &self.trusts {
            WildCardContainer::Wildcard => return true,
            WildCardContainer::List(trusts) => {
                if trusts.iter().any(|descriptor| {
                    descriptor.matches_contract(target_hash, &target_manifest.groups)
                }) {
                    return true;
                }
            }
        }

        self.permissions
            .iter()
            .any(|permission| permission.is_allowed(target_manifest, target_hash, target_method))
    }

    /// Gets a method from the ABI by name.
    pub fn get_method(&self, name: &str) -> Option<&crate::ContractMethodDescriptor> {
        self.abi.methods.iter().find(|m| m.name == name)
    }

    /// Checks if the contract supports a specific standard.
    pub fn supports_standard(&self, standard: &str) -> bool {
        self.supported_standards.contains(&standard.to_string())
    }
}

impl Default for ContractManifest {
    fn default() -> Self {
        Self {
            name: "DefaultContract".to_string(),
            groups: Vec::new(),
            features: ManifestFeatures::empty(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::Wildcard,
            extra: None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/manifest/contract_manifest.rs"]
mod tests;

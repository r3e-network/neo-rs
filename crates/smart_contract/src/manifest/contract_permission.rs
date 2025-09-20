//!
//! Defines the permissions that a contract requires to call other contracts
//! and access their methods.

use super::{ContractGroup, ContractManifest, WildcardContainer};
use crate::{Error, Result};
use base64::{engine::general_purpose, Engine as _};
use hex;
use neo_config::ADDRESS_SIZE;
use neo_core::UInt160;
use neo_cryptography::ecc::{ECCurve, ECPoint};
use serde::{Deserialize, Serialize};

/// Represents a permission that a contract requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractPermission {
    /// The contract or group that this permission applies to.
    pub contract: ContractPermissionDescriptor,

    /// The methods that are allowed to be called.
    pub methods: WildcardContainer<String>,
}

/// Describes what contract or group a permission applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractPermissionDescriptor {
    /// Wildcard - applies to all contracts.
    Wildcard,
    /// Specific contract hash.
    Hash(UInt160),
    /// Group public key.
    Group(ECPoint),
}

impl ContractPermission {
    /// Creates a new contract permission.
    pub fn new(contract: ContractPermissionDescriptor, methods: WildcardContainer<String>) -> Self {
        Self { contract, methods }
    }

    /// Creates a wildcard permission that allows calling any method on any contract.
    pub fn default_wildcard() -> Self {
        Self {
            contract: ContractPermissionDescriptor::create_wildcard(),
            methods: WildcardContainer::create_wildcard(),
        }
    }

    /// Creates a permission for a specific contract hash.
    pub fn for_contract(hash: UInt160, methods: WildcardContainer<String>) -> Self {
        Self {
            contract: ContractPermissionDescriptor::Hash(hash),
            methods,
        }
    }

    /// Creates a permission for a specific group public key.
    pub fn for_group(public_key: ECPoint, methods: WildcardContainer<String>) -> Self {
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
            WildcardContainer::Wildcard => true,
            WildcardContainer::List(methods) => methods.iter().any(|m| m == method_name),
        }
    }

    /// Gets the size of the permission in bytes.
    pub fn size(&self) -> usize {
        let contract_size = self.contract.size();
        let methods_size = match &self.methods {
            WildcardContainer::Wildcard => 1,
            WildcardContainer::List(methods) => methods.iter().map(|m| m.len()).sum(),
        };
        contract_size + methods_size
    }

    /// Validates the permission.
    pub fn validate(&self) -> Result<()> {
        match &self.contract {
            ContractPermissionDescriptor::Wildcard | ContractPermissionDescriptor::Hash(_) => {}
            ContractPermissionDescriptor::Group(pubkey) => {
                if !pubkey.is_valid() {
                    return Err(Error::InvalidManifest(
                        "Invalid public key in contract permission".to_string(),
                    ));
                }
            }
        }

        match &self.methods {
            WildcardContainer::Wildcard => {}
            WildcardContainer::List(methods) => {
                if methods.is_empty() {
                    return Err(Error::InvalidManifest(
                        "Method list cannot be empty".to_string(),
                    ));
                }

                if methods.iter().any(|m| m.is_empty()) {
                    return Err(Error::InvalidManifest(
                        "Method name cannot be empty".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

impl ContractPermissionDescriptor {
    /// Creates a wildcard descriptor.
    pub fn create_wildcard() -> Self {
        ContractPermissionDescriptor::Wildcard
    }

    /// Returns true if the descriptor is a wildcard.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    /// Returns true if the descriptor is a specific hash.
    pub fn is_hash(&self) -> bool {
        matches!(self, Self::Hash(_))
    }

    /// Returns true if the descriptor is a group.
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }

    /// Estimated size in bytes when serialized.
    pub fn size(&self) -> usize {
        match self {
            Self::Wildcard => 1,
            Self::Hash(_) => ADDRESS_SIZE,
            Self::Group(_) => 33,
        }
    }

    /// Checks whether this descriptor matches the supplied contract information.
    pub fn matches_contract(
        &self,
        contract_hash: &UInt160,
        contract_groups: &[ContractGroup],
    ) -> bool {
        match self {
            Self::Wildcard => true,
            Self::Hash(hash) => hash == contract_hash,
            Self::Group(pub_key) => contract_groups
                .iter()
                .any(|group| &group.pub_key == pub_key),
        }
    }
}

impl Serialize for ContractPermissionDescriptor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Wildcard => serializer.serialize_str("*"),
            Self::Hash(hash) => {
                serializer.serialize_str(&format!("0x{}", hex::encode(hash.as_bytes())))
            }
            Self::Group(point) => {
                let encoded = point
                    .encode_compressed()
                    .map_err(|e| serde::ser::Error::custom(e.to_string()))?;
                serializer.serialize_str(&general_purpose::STANDARD.encode(encoded))
            }
        }
    }
}

impl<'de> Deserialize<'de> for ContractPermissionDescriptor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        if value == "*" {
            return Ok(Self::Wildcard);
        }

        if let Some(hex_str) = value.strip_prefix("0x") {
            let bytes = hex::decode(hex_str)
                .map_err(|e| serde::de::Error::custom(format!("Invalid hash: {}", e)))?;
            if bytes.len() != ADDRESS_SIZE {
                return Err(serde::de::Error::custom(
                    "Invalid UInt160 length in contract descriptor",
                ));
            }
            let hash = UInt160::from_bytes(&bytes)
                .map_err(|e| serde::de::Error::custom(format!("Invalid UInt160: {}", e)))?;
            return Ok(Self::Hash(hash));
        }

        let decoded = general_purpose::STANDARD
            .decode(value.as_bytes())
            .map_err(|e| serde::de::Error::custom(format!("Invalid group descriptor: {}", e)))?;

        let point = ECPoint::decode(&decoded, ECCurve::secp256r1())
            .map_err(|e| serde::de::Error::custom(format!("Invalid group public key: {}", e)))?;

        Ok(Self::Group(point))
    }
}

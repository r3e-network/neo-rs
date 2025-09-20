//! Contract manifest implementation.
//!
//! Represents the manifest of a smart contract which declares the features
//! and permissions it will use when deployed.

use crate::manifest::{
    ContractAbi, ContractGroup, ContractPermission, ContractPermissionDescriptor,
};
use crate::{Error, Result};
use neo_config::{HASH_SIZE, MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE};
use neo_core::UInt160;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Maximum length of a contract manifest in bytes.
pub const MAX_MANIFEST_LENGTH: usize = u16::MAX as usize;

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
    pub features: HashMap<String, Value>,

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
    pub trusts: Vec<ContractPermissionDescriptor>,

    /// Additional metadata.
    #[serde(default)]
    pub extra: Option<Value>,
}

impl ContractManifest {
    /// Creates a new contract manifest.
    pub fn new(name: String) -> Self {
        Self {
            name,
            groups: Vec::new(),
            features: HashMap::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: Vec::new(),
            extra: None,
        }
    }

    /// Creates a new native contract manifest.
    pub fn new_native(name: String) -> Self {
        Self {
            name,
            groups: Vec::new(),
            features: HashMap::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: Vec::new(),
            extra: None,
        }
    }

    /// Gets the size of the manifest in bytes.
    pub fn size(&self) -> usize {
        let groups_size: usize = self.groups.iter().map(ContractGroup::size).sum();
        let features_json = serde_json::to_string(&self.features).unwrap_or_default();
        let supported_standards_size: usize =
            self.supported_standards.iter().map(|s| s.len()).sum();
        let permissions_size: usize = self.permissions.iter().map(ContractPermission::size).sum();
        let trusts_size: usize = self
            .trusts
            .iter()
            .map(ContractPermissionDescriptor::size)
            .sum();
        let extra_json = self
            .extra
            .as_ref()
            .map(|value| serde_json::to_string(value).unwrap_or_default())
            .unwrap_or_default();

        self.name.len()
            + 1
            + groups_size
            + 1
            + features_json.len()
            + 1
            + supported_standards_size
            + 1
            + self.abi.size()
            + permissions_size
            + 1
            + trusts_size
            + 1
            + if self.extra.is_some() {
                extra_json.len() + 1
            } else {
                1
            }
    }

    /// Converts the manifest to JSON.
    pub fn to_json(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|e| Error::SerializationError(e.to_string()))
    }

    /// Creates a manifest from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| Error::SerializationError(e.to_string()))
    }

    /// Alias to maintain backwards compatibility with older code paths.
    pub fn from_json(json: &str) -> Result<Self> {
        Self::from_json_str(json)
    }

    /// Parses a contract manifest from JSON.
    /// This is an alias for `from_json_str` to match C# `ContractManifest.Parse` exactly.
    pub fn parse(json: &str) -> Result<Self> {
        Self::from_json_str(json)
    }

    /// Validates the manifest.
    pub fn validate(&self) -> Result<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(Error::InvalidManifest(
                "Contract name cannot be empty".to_string(),
            ));
        }

        // Validate manifest size
        if self.size() > MAX_MANIFEST_LENGTH {
            return Err(Error::InvalidManifest("Manifest too large".to_string()));
        }

        // Validate groups
        for group in &self.groups {
            group.validate()?;
        }

        // Validate permissions
        if self.permissions.is_empty() {
            return Err(Error::InvalidManifest(
                "At least one permission required".to_string(),
            ));
        }

        for permission in &self.permissions {
            permission.validate()?;
        }

        for trust in &self.trusts {
            if let ContractPermissionDescriptor::Group(pub_key) = trust {
                if !pub_key.is_valid() {
                    return Err(Error::InvalidManifest(
                        "Invalid group public key in trusts".to_string(),
                    ));
                }
            }
        }

        // Validate ABI
        self.abi.validate()?;

        Ok(())
    }

    /// Checks if the contract can call another contract.
    pub fn can_call(
        &self,
        target_manifest: &ContractManifest,
        target_hash: &UInt160,
        target_method: &str,
    ) -> bool {
        if self
            .trusts
            .iter()
            .any(|descriptor| descriptor.matches_contract(target_hash, &target_manifest.groups))
        {
            return true;
        }

        self.permissions
            .iter()
            .any(|permission| permission.is_allowed(target_manifest, target_hash, target_method))
    }

    /// Gets a method from the ABI by name.
    pub fn get_method(&self, name: &str) -> Option<&crate::manifest::ContractMethod> {
        self.abi.methods.iter().find(|m| m.name == name)
    }

    /// Checks if the contract supports a specific standard.
    pub fn supports_standard(&self, standard: &str) -> bool {
        self.supported_standards.contains(&standard.to_string())
    }

    /// Serializes the contract manifest to bytes.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<()> {
        // Serialize name
        writer.write_var_string(&self.name)?;

        // Serialize groups
        writer.write_var_int(self.groups.len() as u64)?;
        for group in &self.groups {
            self.serialize_contract_group(group, writer)?;
        }

        let features_json = serde_json::to_string(&self.features)
            .map_err(|e| Error::SerializationError(e.to_string()))?;
        writer.write_var_string(&features_json)?;

        // Serialize supported standards
        writer.write_var_int(self.supported_standards.len() as u64)?;
        for standard in &self.supported_standards {
            writer.write_var_string(standard)?;
        }

        // Serialize ABI using custom serialization
        self.serialize_contract_abi(&self.abi, writer)?;

        // Serialize permissions
        writer.write_var_int(self.permissions.len() as u64)?;
        for permission in &self.permissions {
            self.serialize_contract_permission(permission, writer)?;
        }

        // Serialize trusts
        writer.write_var_int(self.trusts.len() as u64)?;
        for trust in &self.trusts {
            let trust_json = serde_json::to_string(trust)
                .map_err(|e| Error::SerializationError(e.to_string()))?;
            writer.write_var_string(&trust_json)?;
        }

        let extra_json = match &self.extra {
            Some(value) => serde_json::to_string(value)
                .map_err(|e| Error::SerializationError(e.to_string()))?,
            None => String::new(),
        };
        writer.write_var_string(&extra_json)?;

        Ok(())
    }

    /// Deserializes the contract manifest from bytes.
    pub fn deserialize(reader: &mut MemoryReader) -> Result<Self> {
        // Deserialize name
        let name = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max MAX_SCRIPT_SIZE chars for name

        // Deserialize groups
        let groups_count = reader.read_var_int(256)? as usize; // Max 256 groups
        let mut groups = Vec::with_capacity(groups_count);
        for _ in 0..groups_count {
            let group = Self::deserialize_contract_group(reader)?;
            groups.push(group);
        }

        // Deserialize features
        let features_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?; // Max 64KB for features
        let features = serde_json::from_str(&features_json)
            .map_err(|e| Error::SerializationError(e.to_string()))?;

        // Deserialize supported standards
        let standards_count = reader.read_var_int(256)? as usize; // Max 256 standards
        let mut supported_standards = Vec::with_capacity(standards_count);
        for _ in 0..standards_count {
            let standard = reader.read_var_string(256)?; // Max 256 chars per standard
            supported_standards.push(standard);
        }

        // Deserialize ABI
        let abi = Self::deserialize_contract_abi(reader)?;

        // Deserialize permissions
        let permissions_count = reader.read_var_int(256)? as usize; // Max 256 permissions
        let mut permissions = Vec::with_capacity(permissions_count);
        for _ in 0..permissions_count {
            let permission = Self::deserialize_contract_permission(reader)?;
            permissions.push(permission);
        }

        // Deserialize trusts
        let trusts_count = reader.read_var_int(256)? as usize;
        let mut trusts = Vec::with_capacity(trusts_count);
        for _ in 0..trusts_count {
            let trust_json = reader.read_var_string(MAX_SCRIPT_SIZE)?;
            let trust = serde_json::from_str(&trust_json)
                .map_err(|e| Error::SerializationError(e.to_string()))?;
            trusts.push(trust);
        }

        // Deserialize extra
        let extra_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?; // Max 64KB for extra
        let extra = if extra_json.is_empty() {
            None
        } else {
            Some(
                serde_json::from_str(&extra_json)
                    .map_err(|e| Error::SerializationError(e.to_string()))?,
            )
        };

        Ok(Self {
            name,
            groups,
            features,
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        })
    }

    /// Custom serialization for ContractGroup (matches C# ContractGroup.ToStackItem exactly)
    fn serialize_contract_group(
        &self,
        group: &ContractGroup,
        writer: &mut BinaryWriter,
    ) -> Result<()> {
        let group_json =
            serde_json::to_string(group).map_err(|e| Error::SerializationError(e.to_string()))?;
        writer.write_var_string(&group_json)?;

        Ok(())
    }

    /// Custom deserialization for ContractGroup
    fn deserialize_contract_group(reader: &mut MemoryReader) -> Result<ContractGroup> {
        let group_json = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max 1KB per group
        let group = serde_json::from_str(&group_json)
            .map_err(|e| Error::SerializationError(e.to_string()))?;
        Ok(group)
    }

    /// Custom serialization for ContractAbi (matches C# ContractAbi.ToStackItem exactly)
    fn serialize_contract_abi(&self, abi: &ContractAbi, writer: &mut BinaryWriter) -> Result<()> {
        let abi_json =
            serde_json::to_string(abi).map_err(|e| Error::SerializationError(e.to_string()))?;
        writer.write_var_string(&abi_json)?;
        Ok(())
    }

    /// Custom deserialization for ContractAbi
    fn deserialize_contract_abi(reader: &mut MemoryReader) -> Result<ContractAbi> {
        let abi_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?; // Max 64KB for ABI
        let abi = serde_json::from_str(&abi_json)
            .map_err(|e| Error::SerializationError(e.to_string()))?;
        Ok(abi)
    }

    /// Custom serialization for ContractPermission (matches C# ContractPermission.ToStackItem exactly)
    fn serialize_contract_permission(
        &self,
        permission: &ContractPermission,
        writer: &mut BinaryWriter,
    ) -> Result<()> {
        let permission_json = serde_json::to_string(permission)
            .map_err(|e| Error::SerializationError(e.to_string()))?;
        writer.write_var_string(&permission_json)?;
        Ok(())
    }

    /// Custom deserialization for ContractPermission
    fn deserialize_contract_permission(reader: &mut MemoryReader) -> Result<ContractPermission> {
        let permission_json = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max 1KB per permission
        let permission = serde_json::from_str(&permission_json)
            .map_err(|e| Error::SerializationError(e.to_string()))?;
        Ok(permission)
    }
}

impl Default for ContractManifest {
    fn default() -> Self {
        Self {
            name: "DefaultContract".to_string(),
            groups: Vec::new(),
            features: HashMap::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi::default(),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: Vec::new(),
            extra: None,
        }
    }
}

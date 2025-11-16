#![allow(clippy::mutable_key_type)]

//! Contract manifest implementation.
//!
//! Represents the manifest of a smart contract which declares the features
//! and permissions it will use when deployed.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::neo_config::{MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::{
    ContractAbi, ContractGroup, ContractPermission, ContractPermissionDescriptor, WildCardContainer,
};
use crate::UInt160;
use neo_vm::StackItem;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::collections::{BTreeMap, HashMap};

/// Maximum length of a contract manifest in bytes.
pub const MAX_MANIFEST_LENGTH: usize = u16::MAX as usize;

fn map_json_error(err: serde_json::Error) -> IoError {
    IoError::invalid_data(err.to_string())
}

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
    pub trusts: WildCardContainer<ContractPermissionDescriptor>,

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
            trusts: WildCardContainer::create_wildcard(),
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
            trusts: WildCardContainer::create_wildcard(),
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
        let trusts_size: usize = match &self.trusts {
            WildCardContainer::Wildcard => 0,
            WildCardContainer::List(trusts) => {
                trusts.iter().map(ContractPermissionDescriptor::size).sum()
            }
        };
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
        serde_json::to_value(self).map_err(|e| Error::serialization(e.to_string()))
    }

    /// Creates a manifest from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| Error::serialization(e.to_string()))
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
            return Err(Error::invalid_data("Contract name cannot be empty"));
        }

        // Validate manifest size
        if self.size() > MAX_MANIFEST_LENGTH {
            return Err(Error::invalid_data(
                "Manifest exceeds maximum allowed length",
            ));
        }

        // Validate groups
        for group in &self.groups {
            group.validate()?;
        }

        // Validate permissions
        if self.permissions.is_empty() {
            return Err(Error::invalid_data("At least one permission required"));
        }

        for permission in &self.permissions {
            permission.validate()?;
        }

        if let WildCardContainer::List(trusts) = &self.trusts {
            for trust in trusts {
                if let ContractPermissionDescriptor::Group(pub_key) = trust {
                    if !pub_key.is_valid() {
                        return Err(Error::invalid_data("Invalid group public key in trusts"));
                    }
                }
            }
        }

        // Validate ABI
        self.abi.validate().map_err(Error::invalid_data)?;

        Ok(())
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
    pub fn get_method(
        &self,
        name: &str,
    ) -> Option<
        &crate::smart_contract::manifest::contract_method_descriptor::ContractMethodDescriptor,
    > {
        self.abi.methods.iter().find(|m| m.name == name)
    }

    /// Checks if the contract supports a specific standard.
    pub fn supports_standard(&self, standard: &str) -> bool {
        self.supported_standards.contains(&standard.to_string())
    }

    /// Serializes the contract manifest to bytes.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<()> {
        self.serialize_io(writer)
            .map_err(|e| Error::serialization(e.to_string()))
    }

    fn serialize_io(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_var_string(&self.name)?;

        writer.write_var_int(self.groups.len() as u64)?;
        for group in &self.groups {
            self.serialize_contract_group(group, writer)?;
        }

        let features_json = serde_json::to_string(&self.features).map_err(map_json_error)?;
        writer.write_var_string(&features_json)?;

        writer.write_var_int(self.supported_standards.len() as u64)?;
        for standard in &self.supported_standards {
            writer.write_var_string(standard)?;
        }

        self.serialize_contract_abi(&self.abi, writer)?;

        writer.write_var_int(self.permissions.len() as u64)?;
        for permission in &self.permissions {
            self.serialize_contract_permission(permission, writer)?;
        }

        match &self.trusts {
            WildCardContainer::Wildcard => writer.write_var_int(0)?,
            WildCardContainer::List(trusts) => {
                writer.write_var_int(trusts.len() as u64)?;
                for trust in trusts {
                    let trust_json = serde_json::to_string(trust).map_err(map_json_error)?;
                    writer.write_var_string(&trust_json)?;
                }
            }
        }

        let extra_json = match &self.extra {
            Some(value) => serde_json::to_string(value).map_err(map_json_error)?,
            None => String::new(),
        };
        writer.write_var_string(&extra_json)?;

        Ok(())
    }

    /// Deserializes the contract manifest from bytes.
    pub fn deserialize(reader: &mut MemoryReader) -> Result<Self> {
        Self::deserialize_io(reader).map_err(|e| Error::serialization(e.to_string()))
    }

    fn deserialize_io(reader: &mut MemoryReader) -> IoResult<Self> {
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
        let features = serde_json::from_str(&features_json).map_err(map_json_error)?;

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
        let trusts = if trusts_count == 0 {
            WildCardContainer::create_wildcard()
        } else {
            let mut entries = Vec::with_capacity(trusts_count);
            for _ in 0..trusts_count {
                let trust_json = reader.read_var_string(MAX_SCRIPT_SIZE)?;
                let trust: ContractPermissionDescriptor =
                    serde_json::from_str(&trust_json).map_err(map_json_error)?;
                entries.push(trust);
            }
            WildCardContainer::create(entries)
        };

        // Deserialize extra
        let extra_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?; // Max 64KB for extra
        let extra = if extra_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(&extra_json).map_err(map_json_error)?)
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
    ) -> IoResult<()> {
        let group_json = serde_json::to_string(group).map_err(map_json_error)?;
        writer.write_var_string(&group_json)?;

        Ok(())
    }

    /// Custom deserialization for ContractGroup
    fn deserialize_contract_group(reader: &mut MemoryReader) -> IoResult<ContractGroup> {
        let group_json = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max 1KB per group
        let group = serde_json::from_str(&group_json).map_err(map_json_error)?;
        Ok(group)
    }

    /// Custom serialization for ContractAbi (matches C# ContractAbi.ToStackItem exactly)
    fn serialize_contract_abi(&self, abi: &ContractAbi, writer: &mut BinaryWriter) -> IoResult<()> {
        let abi_json = serde_json::to_string(abi).map_err(map_json_error)?;
        writer.write_var_string(&abi_json)?;
        Ok(())
    }

    /// Custom deserialization for ContractAbi
    fn deserialize_contract_abi(reader: &mut MemoryReader) -> IoResult<ContractAbi> {
        let abi_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?; // Max 64KB for ABI
        let abi = serde_json::from_str(&abi_json).map_err(map_json_error)?;
        Ok(abi)
    }

    /// Custom serialization for ContractPermission (matches C# ContractPermission.ToStackItem exactly)
    fn serialize_contract_permission(
        &self,
        permission: &ContractPermission,
        writer: &mut BinaryWriter,
    ) -> IoResult<()> {
        let permission_json = serde_json::to_string(permission).map_err(map_json_error)?;
        writer.write_var_string(&permission_json)?;
        Ok(())
    }

    /// Custom deserialization for ContractPermission
    fn deserialize_contract_permission(reader: &mut MemoryReader) -> IoResult<ContractPermission> {
        let permission_json = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max 1KB per permission
        let permission = serde_json::from_str(&permission_json).map_err(map_json_error)?;
        Ok(permission)
    }
}

impl Serializable for ContractManifest {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Self::deserialize_io(reader)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_io(writer)
    }

    fn size(&self) -> usize {
        self.size()
    }
}

impl IInteroperable for ContractManifest {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        let struct_item = match stack_item {
            StackItem::Struct(struct_item) => struct_item,
            other => panic!(
                "ContractManifest expects struct stack item, found {:?}",
                other.stack_item_type()
            ),
        };

        let items = struct_item.items();
        if items.len() < 8 {
            panic!("ContractManifest stack item must contain eight elements");
        }

        let name_bytes = items[0]
            .as_bytes()
            .expect("ContractManifest name must be byte string");
        self.name = String::from_utf8(name_bytes)
            .unwrap_or_else(|_| panic!("ContractManifest name must be valid UTF-8"));

        self.groups = match &items[1] {
            StackItem::Array(array) => array
                .items()
                .iter()
                .map(ContractGroup::from_stack_item_value)
                .collect(),
            _ => panic!("ContractManifest groups must be an array"),
        };

        // Features map is reserved in C# and currently unused; expect empty map.
        if let StackItem::Map(map_item) = &items[2] {
            if !map_item.items().is_empty() {
                panic!("ContractManifest features map must be empty");
            }
        } else {
            panic!("ContractManifest features must be a map");
        }
        self.features.clear();

        self.supported_standards = match &items[3] {
            StackItem::Array(array) => array
                .items()
                .iter()
                .map(|item| {
                    let bytes = item
                        .as_bytes()
                        .expect("Supported standard must be byte string");
                    String::from_utf8(bytes)
                        .unwrap_or_else(|_| panic!("Supported standard must be UTF-8"))
                })
                .collect(),
            _ => panic!("ContractManifest supported standards must be an array"),
        };

        let mut abi = ContractAbi::default();
        abi.from_stack_item(items[4].clone());
        self.abi = abi;

        self.permissions = match &items[5] {
            StackItem::Array(array) => array
                .items()
                .iter()
                .map(|item| {
                    let mut permission = ContractPermission::default_wildcard();
                    permission.from_stack_item(item.clone());
                    permission
                })
                .collect(),
            _ => panic!("ContractManifest permissions must be an array"),
        };

        self.trusts = match &items[6] {
            StackItem::Null => WildCardContainer::create_wildcard(),
            StackItem::Array(array) => WildCardContainer::create(
                array
                    .items()
                    .iter()
                    .map(|item| {
                        ContractPermissionDescriptor::from_stack_item(item)
                            .expect("Invalid contract descriptor in trusts")
                    })
                    .collect(),
            ),
            _ => panic!("ContractManifest trusts must be null or array"),
        };

        self.extra = match &items[7] {
            StackItem::Null => None,
            StackItem::ByteString(bytes) => parse_extra_bytes(bytes.as_slice()),
            StackItem::Buffer(buffer) => parse_extra_bytes(buffer.data()),
            other => panic!(
                "ContractManifest extra must be byte string or null, found {:?}",
                other.stack_item_type()
            ),
        };
    }

    fn to_stack_item(&self) -> StackItem {
        let group_items = self
            .groups
            .iter()
            .map(|group| group.to_stack_item())
            .collect::<Vec<_>>();

        let mut features_map = BTreeMap::new();
        for (key, value) in &self.features {
            let json_text = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
            features_map.insert(
                StackItem::from_byte_string(key.as_bytes()),
                StackItem::from_byte_string(json_text.into_bytes()),
            );
        }

        let standards_items = self
            .supported_standards
            .iter()
            .map(|standard| StackItem::from_byte_string(standard.as_bytes()))
            .collect::<Vec<_>>();

        let permission_items = self
            .permissions
            .iter()
            .map(|permission| permission.to_stack_item())
            .collect::<Vec<_>>();

        let trusts_item = match &self.trusts {
            WildCardContainer::Wildcard => StackItem::null(),
            WildCardContainer::List(trusts) => {
                let items = trusts
                    .iter()
                    .map(|trust| trust.to_stack_item())
                    .collect::<Vec<_>>();
                StackItem::from_array(items)
            }
        };

        let extra_bytes = match &self.extra {
            Some(extra) => serde_json::to_string(extra)
                .unwrap_or_else(|_| "null".to_string())
                .into_bytes(),
            None => "null".as_bytes().to_vec(),
        };

        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes()),
            StackItem::from_array(group_items),
            StackItem::from_map(features_map),
            StackItem::from_array(standards_items),
            self.abi.to_stack_item(),
            StackItem::from_array(permission_items),
            trusts_item,
            StackItem::from_byte_string(extra_bytes),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
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
            trusts: WildCardContainer::Wildcard,
            extra: None,
        }
    }
}

fn parse_extra_bytes(bytes: &[u8]) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }

    let text = std::str::from_utf8(bytes)
        .unwrap_or_else(|_| panic!("ContractManifest extra must be UTF-8"));

    if text == "null" {
        None
    } else {
        Some(
            serde_json::from_str(text).unwrap_or_else(|_| panic!("Invalid JSON in manifest extra")),
        )
    }
}

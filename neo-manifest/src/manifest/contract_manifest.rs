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
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_primitives::constants::{MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE};
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::collections::{HashMap, HashSet};

/// Maximum length of a contract manifest in bytes.
pub const MAX_MANIFEST_LENGTH: usize = u16::MAX as usize;

fn map_json_error(err: serde_json::Error) -> IoError {
    IoError::invalid_data(err.to_string())
}

fn write_json_item<T: serde::Serialize>(item: &T, writer: &mut BinaryWriter) -> IoResult<()> {
    let json = serde_json::to_string(item).map_err(map_json_error)?;
    writer.write_var_string(&json)
}

fn read_json_item<T: for<'de> serde::Deserialize<'de>>(
    reader: &mut MemoryReader,
    max_len: usize,
) -> IoResult<T> {
    let json = reader.read_var_string(max_len)?;
    serde_json::from_str(&json).map_err(map_json_error)
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
            trusts: WildCardContainer::default(),
            extra: None,
        }
    }

    /// Gets the size of the manifest in bytes.
    pub fn size(&self) -> usize {
        // NOTE: The binary manifest format embeds several fields as JSON strings (matching the C#
        // node). Size calculations must mirror `serialize_io` exactly.
        let mut size = 0usize;

        size += SerializeHelper::get_var_size_str(&self.name);

        size += SerializeHelper::get_var_size(self.groups.len() as u64);
        for group in &self.groups {
            let group_json = serde_json::to_string(group).unwrap_or_default();
            size += SerializeHelper::get_var_size_str(&group_json);
        }

        let features_json = serde_json::to_string(&self.features).unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&features_json);

        size += SerializeHelper::get_var_size(self.supported_standards.len() as u64);
        for standard in &self.supported_standards {
            size += SerializeHelper::get_var_size_str(standard);
        }

        let abi_json = serde_json::to_string(&self.abi).unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&abi_json);

        size += SerializeHelper::get_var_size(self.permissions.len() as u64);
        for permission in &self.permissions {
            let permission_json = serde_json::to_string(permission).unwrap_or_default();
            size += SerializeHelper::get_var_size_str(&permission_json);
        }

        match &self.trusts {
            WildCardContainer::Wildcard => size += SerializeHelper::get_var_size(0),
            WildCardContainer::List(trusts) => {
                size += SerializeHelper::get_var_size(trusts.len() as u64);
                for trust in trusts {
                    let trust_json = serde_json::to_string(trust).unwrap_or_default();
                    size += SerializeHelper::get_var_size_str(&trust_json);
                }
            }
        }

        let extra_json = self
            .extra
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&extra_json);

        size
    }

    /// Converts the manifest to JSON.
    pub fn to_json(&self) -> CoreResult<Value> {
        serde_json::to_value(self).map_err(|e| CoreError::serialization(e.to_string()))
    }

    /// Creates a manifest from a JSON string.
    pub fn from_json_str(json: &str) -> CoreResult<Self> {
        if json.len() > MAX_MANIFEST_LENGTH {
            return Err(CoreError::invalid_data(format!(
                "JSON content length {} exceeds maximum allowed size of {} bytes",
                json.len(),
                MAX_MANIFEST_LENGTH
            )));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|e| CoreError::serialization(e.to_string()))?;
        Self::from_json_value(&value)
    }

    /// Alias to maintain backwards compatibility with older code paths.
    pub fn from_json(json: &str) -> CoreResult<Self> {
        Self::from_json_str(json)
    }

    /// Parses a contract manifest from JSON.
    /// This is an alias for `from_json_str` to match C# `ContractManifest.Parse` exactly.
    pub fn parse(json: &str) -> CoreResult<Self> {
        Self::from_json_str(json)
    }

    fn from_json_value(json: &Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected manifest object"))?;

        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::other("Missing name"))?
            .to_string();
        if name.is_empty() {
            return Err(CoreError::other("Name in ContractManifest is empty"));
        }

        let groups = match obj.get("groups") {
            Some(Value::Array(groups)) => groups
                .iter()
                .map(ContractGroup::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => return Err(CoreError::other("ContractManifest groups must be an array")),
            None => Vec::new(),
        };

        let features = obj
            .get("features")
            .and_then(Value::as_object)
            .ok_or_else(|| CoreError::other("Features field must be empty"))?;
        if !features.is_empty() {
            return Err(CoreError::other("Features field must be empty"));
        }

        let supported_standards = match obj.get("supportedstandards") {
            Some(Value::Array(standards)) => standards
                .iter()
                .map(|standard| {
                    let value = standard.as_str().ok_or_else(|| {
                        CoreError::other("SupportedStandards in ContractManifest must be strings")
                    })?;
                    if value.is_empty() {
                        return Err(CoreError::other(
                            "SupportedStandards in ContractManifest has empty string",
                        ));
                    }
                    Ok(value.to_string())
                })
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => {
                return Err(CoreError::other(
                    "SupportedStandards in ContractManifest must be an array",
                ));
            }
            None => Vec::new(),
        };

        let abi = ContractAbi::from_json(
            obj.get("abi")
                .ok_or_else(|| CoreError::other("Missing abi"))?,
        )?;

        let permissions = match obj.get("permissions") {
            Some(Value::Array(permissions)) => permissions
                .iter()
                .map(ContractPermission::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => {
                return Err(CoreError::other(
                    "ContractManifest permissions must be an array",
                ));
            }
            None => Vec::new(),
        };

        let trusts = WildCardContainer::<ContractPermissionDescriptor>::from_json(
            obj.get("trusts")
                .ok_or_else(|| CoreError::other("Missing trusts"))?,
        )?;

        let extra = match obj.get("extra") {
            None | Some(Value::Null) => None,
            Some(value @ Value::Object(_)) => Some(value.clone()),
            Some(_) => return Err(CoreError::other("ContractManifest extra must be an object")),
        };

        let manifest = Self {
            name,
            groups,
            features: HashMap::new(),
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        };

        manifest.validate_manifest_json_uniqueness()?;
        Ok(manifest)
    }

    fn validate_manifest_json_uniqueness(&self) -> CoreResult<()> {
        let mut group_keys = Vec::new();
        for group in &self.groups {
            if group_keys.iter().any(|key| key == &group.pub_key) {
                return Err(CoreError::other("Duplicate group public key in manifest"));
            }
            group_keys.push(group.pub_key.clone());
        }

        let mut standards = HashSet::new();
        for standard in &self.supported_standards {
            if !standards.insert(standard) {
                return Err(CoreError::other("Supported standards must be unique"));
            }
        }

        let mut permission_contracts = Vec::new();
        for permission in &self.permissions {
            if permission_contracts
                .iter()
                .any(|contract| contract == &permission.contract)
            {
                return Err(CoreError::other(
                    "Duplicate permission contract in manifest",
                ));
            }
            permission_contracts.push(permission.contract.clone());
        }

        let mut trusts = Vec::new();
        if let WildCardContainer::List(items) = &self.trusts {
            for item in items {
                if trusts.iter().any(|existing| existing == item) {
                    return Err(CoreError::other("Duplicate trust entry in manifest"));
                }
                trusts.push(item.clone());
            }
        }

        Ok(())
    }

    /// Validates the manifest.
    pub fn validate(&self) -> CoreResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(CoreError::invalid_data("Contract name cannot be empty"));
        }

        if !self.features.is_empty() {
            return Err(CoreError::invalid_data("Features field must be empty"));
        }

        let mut seen_standards = HashSet::new();
        for standard in &self.supported_standards {
            if standard.is_empty() {
                return Err(CoreError::invalid_data(
                    "Supported standards cannot include empty strings",
                ));
            }
            if !seen_standards.insert(standard) {
                return Err(CoreError::invalid_data(
                    "Supported standards must be unique",
                ));
            }
        }

        // Validate manifest size
        if self.size() > MAX_MANIFEST_LENGTH {
            return Err(CoreError::invalid_data(
                "Manifest exceeds maximum allowed length",
            ));
        }

        // Validate groups
        let mut group_keys = Vec::new();
        for group in &self.groups {
            group.validate()?;
            if group_keys.iter().any(|key| key == &group.pub_key) {
                return Err(CoreError::invalid_data(
                    "Duplicate group public key in manifest",
                ));
            }
            group_keys.push(group.pub_key.clone());
        }

        // Validate permissions. Neo N3 allows empty permissions arrays, which
        // means the contract is not allowed to call any external methods.
        let mut permission_contracts = Vec::new();
        for permission in &self.permissions {
            permission.validate()?;
            if permission_contracts
                .iter()
                .any(|contract| contract == &permission.contract)
            {
                return Err(CoreError::invalid_data(
                    "Duplicate permission contract in manifest",
                ));
            }
            permission_contracts.push(permission.contract.clone());
        }

        if let WildCardContainer::List(trusts) = &self.trusts {
            let mut seen_trusts = Vec::new();
            for trust in trusts {
                if seen_trusts.iter().any(|existing| existing == trust) {
                    return Err(CoreError::invalid_data("Duplicate trust entry in manifest"));
                }
                seen_trusts.push(trust.clone());
                if let ContractPermissionDescriptor::Group(pub_key) = trust {
                    if !pub_key.is_valid() {
                        return Err(CoreError::invalid_data(
                            "Invalid group public key in trusts",
                        ));
                    }
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

    /// Serializes the contract manifest to bytes.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> CoreResult<()> {
        self.serialize_io(writer)
            .map_err(|e| CoreError::serialization(e.to_string()))
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
    pub fn deserialize(reader: &mut MemoryReader) -> CoreResult<Self> {
        Self::deserialize_io(reader).map_err(|e| CoreError::serialization(e.to_string()))
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

    fn serialize_contract_group(
        &self,
        group: &ContractGroup,
        writer: &mut BinaryWriter,
    ) -> IoResult<()> {
        write_json_item(group, writer)
    }

    fn deserialize_contract_group(reader: &mut MemoryReader) -> IoResult<ContractGroup> {
        read_json_item(reader, MAX_SCRIPT_SIZE)
    }

    fn serialize_contract_abi(&self, abi: &ContractAbi, writer: &mut BinaryWriter) -> IoResult<()> {
        write_json_item(abi, writer)
    }

    fn deserialize_contract_abi(reader: &mut MemoryReader) -> IoResult<ContractAbi> {
        read_json_item(reader, MAX_SCRIPT_LENGTH)
    }

    fn serialize_contract_permission(
        &self,
        permission: &ContractPermission,
        writer: &mut BinaryWriter,
    ) -> IoResult<()> {
        write_json_item(permission, writer)
    }

    fn deserialize_contract_permission(reader: &mut MemoryReader) -> IoResult<ContractPermission> {
        read_json_item(reader, MAX_SCRIPT_SIZE)
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

impl Interoperable for ContractManifest {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        self.from_stack_value(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl ContractManifest {
    /// Converts the manifest to the VM stack-value shape used by native interop.
    pub fn to_stack_value(&self) -> StackValue {
        let group_items = self
            .groups
            .iter()
            .map(ContractGroup::to_stack_value)
            .collect::<Vec<_>>();

        let standards_items = self
            .supported_standards
            .iter()
            .map(|standard| StackValue::ByteString(standard.as_bytes().to_vec()))
            .collect::<Vec<_>>();

        let permission_items = self
            .permissions
            .iter()
            .map(ContractPermission::to_stack_value)
            .collect::<Vec<_>>();

        let trusts_item = match &self.trusts {
            WildCardContainer::Wildcard => StackValue::Null,
            WildCardContainer::List(trusts) => StackValue::Array(
                0,
                trusts
                    .iter()
                    .map(ContractPermissionDescriptor::to_stack_value)
                    .collect(),
            ),
        };

        let extra_bytes = match &self.extra {
            Some(extra) => neo_serialization::JsonSerializer::encode_value_csharp_compatible(extra),
            None => b"null".to_vec(),
        };

        StackValue::Struct(
            0,
            vec![
                StackValue::ByteString(self.name.as_bytes().to_vec()),
                StackValue::Array(0, group_items),
                // C# ContractManifest.ToStackItem always emits an empty features map.
                StackValue::Map(0, Vec::new()),
                StackValue::Array(0, standards_items),
                self.abi.to_stack_value(),
                StackValue::Array(0, permission_items),
                trusts_item,
                StackValue::ByteString(extra_bytes),
            ],
        )
    }

    /// Populates the manifest from the VM stack-value shape used by native interop.
    pub fn from_stack_value(
        &mut self,
        stack_value: StackValue,
    ) -> std::result::Result<(), CoreError> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_format(
                "ContractManifest expects Struct stack value",
            ));
        };

        if items.len() < 8 {
            return Err(CoreError::invalid_format(format!(
                "ContractManifest stack value must contain 8 elements, found {}",
                items.len()
            )));
        }

        let name_bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_format("ContractManifest name must be ByteString"))?;
        self.name = String::from_utf8(name_bytes)
            .map_err(|_| CoreError::invalid_format("ContractManifest name must be valid UTF-8"))?;

        self.groups = match &items[1] {
            StackValue::Array(0, groups) => {
                let mut values = Vec::with_capacity(groups.len());
                for item in groups {
                    values.push(ContractGroup::try_from_stack_value(item.clone())?);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest groups must be an Array",
                ));
            }
        };

        if let StackValue::Map(0, features) = &items[2] {
            if !features.is_empty() {
                return Err(CoreError::invalid_format(
                    "ContractManifest features map must be empty",
                ));
            }
        } else {
            return Err(CoreError::invalid_format(
                "ContractManifest features must be a Map",
            ));
        }
        self.features.clear();

        self.supported_standards = match &items[3] {
            StackValue::Array(0, standards) => {
                let mut values = Vec::with_capacity(standards.len());
                for item in standards {
                    if matches!(item, StackValue::Null) {
                        return Err(CoreError::invalid_format(
                            "ContractManifest supported standard must not be null",
                        ));
                    }
                    let bytes = item.to_byte_string_bytes().ok_or_else(|| {
                        CoreError::invalid_format(
                            "ContractManifest supported standard must be ByteString",
                        )
                    })?;
                    let standard = String::from_utf8(bytes).map_err(|_| {
                        CoreError::invalid_format(
                            "ContractManifest supported standard must be valid UTF-8",
                        )
                    })?;
                    values.push(standard);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest supported standards must be an Array",
                ));
            }
        };

        let mut abi = ContractAbi::default();
        abi.from_stack_value(items[4].clone())?;
        self.abi = abi;

        self.permissions = match &items[5] {
            StackValue::Array(0, permissions) => {
                let mut values = Vec::new();
                for item in permissions {
                    let mut permission = ContractPermission::default_wildcard();
                    permission.from_stack_value(item.clone())?;
                    values.push(permission);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest permissions must be an Array",
                ));
            }
        };

        self.trusts = match &items[6] {
            StackValue::Null => WildCardContainer::create_wildcard(),
            StackValue::Array(0, trusts) => {
                let mut values = Vec::with_capacity(trusts.len());
                for item in trusts {
                    values.push(ContractPermissionDescriptor::from_stack_value(
                        item.clone(),
                    )?);
                }
                WildCardContainer::create(values)
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest trusts must be Null or Array",
                ));
            }
        };

        self.extra = match &items[7] {
            StackValue::ByteString(bytes) | StackValue::Buffer(0, bytes) => {
                parse_extra_bytes(bytes.as_slice())?
            }
            other => {
                return Err(CoreError::invalid_format(format!(
                    "ContractManifest extra must be ByteString, found {:?}",
                    other.compact_type_tag()
                )));
            }
        };

        Ok(())
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

fn parse_extra_bytes(bytes: &[u8]) -> Result<Option<Value>, CoreError> {
    if bytes.is_empty() {
        return Err(CoreError::invalid_format(
            "ContractManifest extra must not be empty",
        ));
    }

    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return Err(CoreError::invalid_format(
                "ContractManifest extra must be valid UTF-8",
            ));
        }
    };

    match serde_json::from_str::<Value>(text) {
        Ok(Value::Null) => Ok(None),
        Ok(value @ Value::Object(_)) => Ok(Some(value)),
        Ok(_) => Err(CoreError::invalid_format(
            "ContractManifest extra must be a JSON object or null",
        )),
        Err(e) => Err(CoreError::invalid_format(format!(
            "Invalid JSON in manifest extra: {e}"
        ))),
    }
}

#[cfg(test)]
mod manifest_extra_escape_tests {
    use super::*;
    use neo_vm_rs::StackValue;

    fn stack_items_from_manifest(manifest: &ContractManifest) -> Vec<StackValue> {
        let StackValue::Struct(0, items) = manifest.to_stack_value() else {
            panic!("expected manifest Struct")
        };
        items
    }

    fn deployable_manifest_json() -> Value {
        serde_json::json!({
            "name": "sample",
            "groups": [],
            "features": {},
            "supportedstandards": [],
            "abi": {
                "methods": [{
                    "name": "main",
                    "parameters": [],
                    "returntype": "Void",
                    "offset": 0,
                    "safe": false
                }],
                "events": []
            },
            "permissions": [],
            "trusts": "*",
            "extra": null
        })
    }

    /// Bug #10 regression — manifest `extra` JSON must use C# JavaScriptEncoder.Default
    /// escape semantics. serde_json's minimal RFC-8259 escape set produces wrong bytes
    /// for `&`, `<`, `>`, `'`, `+`, `` ` ``, and all non-ASCII. Block 1,208,916 deploy
    /// of "Three Orange Hearts" NEP-11 had `&` in the description; serde_json kept it
    /// literal, C# escaped it to `&`, state roots diverged from that block onward.
    #[test]
    fn extra_with_ampersand_uses_csharp_escape() {
        let m = ContractManifest {
            extra: Some(serde_json::json!({
                "description": "NEO, GAS, & FLM on Neo N3"
            })),
            ..Default::default()
        };
        let value = m.to_stack_value();
        let StackValue::Struct(0, items) = value else {
            panic!("expected Struct")
        };
        let extra_item = &items[7];
        let StackValue::ByteString(extra_bytes) = extra_item else {
            panic!("expected extra ByteString")
        };
        let extra_str = std::str::from_utf8(extra_bytes).expect("utf-8");
        assert!(
            extra_str.contains("\\u0026"),
            "expected `&` to be escaped as `\\u0026`, got: {extra_str}"
        );
        assert!(
            !extra_str.contains('&'),
            "raw `&` must NOT appear in C#-compatible output, got: {extra_str}"
        );
    }

    #[test]
    fn contract_manifest_projects_to_stack_value() {
        let mut manifest = ContractManifest::new("sample".to_string());
        manifest.supported_standards = vec!["NEP-17".to_string()];
        manifest.features.insert(
            "feature".to_string(),
            serde_json::json!({
                "description": "GAS & NEO"
            }),
        );
        manifest.extra = Some(serde_json::json!({
            "description": "NEO, GAS, & FLM on Neo N3"
        }));

        let value = manifest.to_stack_value();
        let StackValue::Struct(0, items) = value else {
            panic!("expected manifest Struct")
        };

        assert_eq!(items[0], StackValue::ByteString(b"sample".to_vec()));
        assert_eq!(items[1], StackValue::Array(0, Vec::new()));
        let StackValue::Map(0, features) = &items[2] else {
            panic!("expected features map")
        };
        assert!(
            features.is_empty(),
            "C# ContractManifest.ToStackItem always emits an empty features map"
        );
        assert_eq!(
            items[3],
            StackValue::Array(0, vec![StackValue::ByteString(b"NEP-17".to_vec())])
        );
        assert_eq!(items[4], manifest.abi.to_stack_value());
        assert_eq!(
            items[5],
            StackValue::Array(0, vec![manifest.permissions[0].to_stack_value()])
        );
        assert_eq!(items[6], StackValue::Null);
        let StackValue::ByteString(extra_bytes) = &items[7] else {
            panic!("expected extra ByteString")
        };
        let extra = std::str::from_utf8(extra_bytes).expect("extra utf8");
        assert!(
            extra.contains("\\u0026"),
            "extra should use C# JSON escapes"
        );
    }

    #[test]
    fn contract_manifest_reads_stack_value() {
        let mut source = ContractManifest::new("sample".to_string());
        source.supported_standards = vec!["NEP-17".to_string()];
        source.extra = Some(serde_json::json!({"description": "ok"}));

        let mut decoded = ContractManifest::default();
        decoded
            .from_stack_value(source.to_stack_value())
            .expect("manifest from stack value");

        assert_eq!(decoded.name, source.name);
        assert_eq!(decoded.supported_standards, source.supported_standards);
        assert_eq!(decoded.abi, source.abi);
        assert_eq!(decoded.permissions, source.permissions);
        assert_eq!(decoded.trusts, source.trusts);
        assert_eq!(decoded.extra, source.extra);
    }

    #[test]
    fn contract_manifest_parse_uses_csharp_json_field_rules() {
        let manifest = ContractManifest::parse(&deployable_manifest_json().to_string())
            .expect("valid manifest parses");
        assert_eq!(manifest.name, "sample");

        let mut empty_methods_permission = deployable_manifest_json();
        empty_methods_permission["permissions"] =
            serde_json::json!([{ "contract": "*", "methods": [] }]);
        let manifest = ContractManifest::parse(&empty_methods_permission.to_string())
            .expect("C# permits empty permission method lists");
        manifest
            .validate()
            .expect("empty method list remains valid");

        let mut bad_parameter = deployable_manifest_json();
        bad_parameter["abi"]["methods"][0]["parameters"] =
            serde_json::json!([{ "name": "bad", "type": "Void" }]);
        assert!(ContractManifest::parse(&bad_parameter.to_string()).is_err());

        let mut missing_features = deployable_manifest_json();
        missing_features.as_object_mut().unwrap().remove("features");
        assert!(ContractManifest::parse(&missing_features.to_string()).is_err());

        let mut missing_trusts = deployable_manifest_json();
        missing_trusts.as_object_mut().unwrap().remove("trusts");
        assert!(ContractManifest::parse(&missing_trusts.to_string()).is_err());
    }

    #[test]
    fn contract_manifest_rejects_non_empty_features_stack_value_like_csharp() {
        let source = ContractManifest::new("sample".to_string());
        let mut items = stack_items_from_manifest(&source);
        items[2] = StackValue::Map(
            0,
            vec![(
                StackValue::ByteString(b"feature".to_vec()),
                StackValue::ByteString(b"{}".to_vec()),
            )],
        );

        assert!(
            ContractManifest::default()
                .from_stack_value(StackValue::Struct(0, items))
                .is_err()
        );
    }

    #[test]
    fn contract_manifest_rejects_malformed_stack_fields_like_csharp() {
        let assert_rejected = |mutate: fn(&mut Vec<StackValue>)| {
            let source = ContractManifest::new("sample".to_string());
            let mut items = stack_items_from_manifest(&source);
            mutate(&mut items);
            assert!(
                ContractManifest::default()
                    .from_stack_value(StackValue::Struct(0, items))
                    .is_err()
            );
        };

        assert_rejected(|items| {
            items[1] = StackValue::Array(0, vec![StackValue::Null]);
        });
        assert_rejected(|items| {
            items[3] = StackValue::Array(0, vec![StackValue::ByteString(vec![0xff])]);
        });
        assert_rejected(|items| {
            items[3] = StackValue::Array(0, vec![StackValue::Null]);
        });
        assert_rejected(|items| {
            items[6] = StackValue::Array(0, vec![StackValue::ByteString(vec![1, 2, 3])]);
        });
        assert_rejected(|items| {
            items[7] = StackValue::Null;
        });
        assert_rejected(|items| {
            items[7] = StackValue::ByteString(b"[]".to_vec());
        });
    }
}

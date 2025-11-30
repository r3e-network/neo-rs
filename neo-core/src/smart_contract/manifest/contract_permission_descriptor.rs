//! ContractPermissionDescriptor - matches C# Neo.SmartContract.Manifest.ContractPermissionDescriptor exactly

use super::contract_group::ContractGroup;
use crate::cryptography::crypto_utils::ECPoint;
use crate::UInt160;
use neo_vm::StackItem;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Describes what contract or group a permission applies to (matches C# ContractPermissionDescriptor)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractPermissionDescriptor {
    /// Wildcard - permission applies to any contract
    Wildcard,
    /// Permission applies to a specific contract hash
    Hash(UInt160),
    /// Permission applies to contracts in a specific group
    Group(ECPoint),
}

impl ContractPermissionDescriptor {
    /// Creates a wildcard descriptor
    pub fn create_wildcard() -> Self {
        ContractPermissionDescriptor::Wildcard
    }

    /// Creates a hash descriptor
    pub fn create_hash(hash: UInt160) -> Self {
        ContractPermissionDescriptor::Hash(hash)
    }

    /// Creates a group descriptor
    pub fn create_group(public_key: ECPoint) -> Self {
        ContractPermissionDescriptor::Group(public_key)
    }

    /// Checks if this is a wildcard
    pub fn is_wildcard(&self) -> bool {
        matches!(self, ContractPermissionDescriptor::Wildcard)
    }

    /// Checks if this descriptor allows the specified hash
    pub fn is_allowed(&self, hash: &UInt160, groups: &[ECPoint]) -> bool {
        match self {
            ContractPermissionDescriptor::Wildcard => true,
            ContractPermissionDescriptor::Hash(h) => h == hash,
            ContractPermissionDescriptor::Group(g) => groups.contains(g),
        }
    }

    /// Checks if this descriptor matches a contract hash using manifest groups.
    pub fn matches_contract(&self, hash: &UInt160, groups: &[ContractGroup]) -> bool {
        match self {
            ContractPermissionDescriptor::Wildcard => true,
            ContractPermissionDescriptor::Hash(h) => h == hash,
            ContractPermissionDescriptor::Group(group_key) => {
                groups.iter().any(|g| &g.pub_key == group_key)
            }
        }
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        if let Some(s) = json.as_str() {
            if s == "*" {
                return Ok(ContractPermissionDescriptor::Wildcard);
            }
            // Try to parse as hash
            if let Ok(hash) = UInt160::parse(s) {
                return Ok(ContractPermissionDescriptor::Hash(hash));
            }
            // Try to parse as public key
            if let Ok(bytes) = hex::decode(s) {
                return Ok(ContractPermissionDescriptor::Group(ECPoint::new(bytes)));
            }
        }
        Err("Invalid permission descriptor".to_string())
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ContractPermissionDescriptor::Wildcard => serde_json::Value::String("*".to_string()),
            ContractPermissionDescriptor::Hash(h) => serde_json::Value::String(h.to_string()),
            ContractPermissionDescriptor::Group(g) => {
                serde_json::Value::String(hex::encode(g.encoded()))
            }
        }
    }

    /// Converts the descriptor to its stack item representation.
    pub fn to_stack_item(&self) -> StackItem {
        match self {
            ContractPermissionDescriptor::Wildcard => StackItem::null(),
            ContractPermissionDescriptor::Hash(hash) => {
                StackItem::from_byte_string(hash.to_bytes())
            }
            ContractPermissionDescriptor::Group(group) => {
                StackItem::from_byte_string(group.encode_point(true).unwrap_or_else(|e| {
                    tracing::error!("Failed to encode group key: {}", e);
                    group.to_bytes()
                }))
            }
        }
    }

    /// Creates a descriptor from a stack item encoded form.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, String> {
        match item {
            StackItem::Null => Ok(Self::create_wildcard()),
            StackItem::ByteString(bytes) => Self::from_bytes(bytes),
            StackItem::Buffer(buffer) => Self::from_bytes(buffer.data()),
            other => Err(format!(
                "Unsupported stack item type for ContractPermissionDescriptor: {:?}",
                other.stack_item_type()
            )),
        }
    }

    /// Builds a descriptor from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        match bytes.len() {
            0 => Ok(Self::create_wildcard()),
            20 => Ok(Self::create_hash(
                UInt160::from_bytes(bytes).map_err(|e| format!("Invalid UInt160 bytes: {}", e))?,
            )),
            33 => Ok(Self::create_group(
                ECPoint::from_bytes(bytes).map_err(|e| format!("Invalid ECPoint bytes: {}", e))?,
            )),
            len => Err(format!("Invalid descriptor byte length: {}", len)),
        }
    }

    /// Approximate serialized size of the descriptor.
    pub fn size(&self) -> usize {
        match self {
            ContractPermissionDescriptor::Wildcard => 1,
            ContractPermissionDescriptor::Hash(_) => 1 + 20,
            ContractPermissionDescriptor::Group(_) => 1 + 33,
        }
    }
}

impl Serialize for ContractPermissionDescriptor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ContractPermissionDescriptor::Wildcard => serializer.serialize_str("*"),
            ContractPermissionDescriptor::Hash(hash) => serializer.serialize_str(&hash.to_string()),
            ContractPermissionDescriptor::Group(group) => {
                serializer.serialize_str(&hex::encode(group.encoded()))
            }
        }
    }
}

impl<'de> Deserialize<'de> for ContractPermissionDescriptor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        ContractPermissionDescriptor::from_json(&value).map_err(serde::de::Error::custom)
    }
}

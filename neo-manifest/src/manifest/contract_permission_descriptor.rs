//! ContractPermissionDescriptor - matches C# Neo.SmartContract.Manifest.ContractPermissionDescriptor exactly

use super::contract_group::ContractGroup;
use neo_crypto::ECPoint;
use neo_primitives::UInt160;
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
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
                let key = ECPoint::from_bytes(&bytes).map_err(|e| e.to_string())?;
                return Ok(ContractPermissionDescriptor::Group(key));
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
    ///
    /// C# reference: `ContractPermissionDescriptor.ToStackItem()`
    /// Wildcard produces `Null` in BinarySerializer output (verified against mainnet state root).
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            ContractPermissionDescriptor::Wildcard => StackValue::Null,
            ContractPermissionDescriptor::Hash(hash) => StackValue::ByteString(hash.to_bytes()),
            ContractPermissionDescriptor::Group(group) => {
                StackValue::ByteString(group.encode_point(true).unwrap_or_else(|e| {
                    tracing::error!("Failed to encode group key: {}", e);
                    group.to_bytes()
                }))
            }
        }
    }

    /// Converts the descriptor to its stack item representation.
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value()).unwrap_or(StackItem::Null)
    }

    /// Creates a descriptor from a neo-vm-rs stack value encoded form.
    pub fn from_stack_value(stack_value: StackValue) -> Result<Self, String> {
        match stack_value {
            StackValue::Null => Ok(Self::create_wildcard()),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => Self::from_bytes(&bytes),
            other => Err(format!(
                "Unsupported stack value type for ContractPermissionDescriptor: {:?}",
                other
            )),
        }
    }

    /// Creates a descriptor from a stack item encoded form.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, String> {
        Self::from_stack_value(StackValue::try_from(item.clone()).map_err(|_| {
            format!(
                "Unsupported stack item type for ContractPermissionDescriptor: {:?}",
                item.stack_item_type()
            )
        })?)
    }

    /// Builds a descriptor from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        // C# encodes Wildcard as ByteString("*") (single byte 0x2A)
        if bytes == b"*" {
            return Ok(Self::create_wildcard());
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::{ECPoint, Secp256r1Crypto};
    use neo_vm_rs::StackValue;

    fn group_key() -> ECPoint {
        let private_key = [1u8; 32];
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
        ECPoint::from_bytes(&public_key).expect("group public key")
    }

    #[test]
    fn permission_descriptor_projects_to_neo_vm_rs_stack_value() {
        let hash = UInt160::from_bytes(&[0x11; 20]).expect("hash");
        let group = group_key();
        let group_bytes = group.encode_point(true).expect("compressed key");

        assert_eq!(
            ContractPermissionDescriptor::create_wildcard().to_stack_value(),
            StackValue::Null
        );
        assert_eq!(
            ContractPermissionDescriptor::create_hash(hash).to_stack_value(),
            StackValue::ByteString(hash.to_bytes())
        );
        assert_eq!(
            ContractPermissionDescriptor::create_group(group).to_stack_value(),
            StackValue::ByteString(group_bytes)
        );
    }

    #[test]
    fn permission_descriptor_stack_item_projection_matches_stack_value_projection() {
        let descriptor =
            ContractPermissionDescriptor::create_hash(UInt160::from_bytes(&[0x22; 20]).unwrap());
        let expected = StackItem::try_from(descriptor.to_stack_value()).unwrap();

        assert_eq!(descriptor.to_stack_item(), expected);
    }

    #[test]
    fn permission_descriptor_reads_from_neo_vm_rs_stack_value() {
        let hash = UInt160::from_bytes(&[0x33; 20]).expect("hash");
        let group = group_key();
        let group_bytes = group.encode_point(true).expect("compressed key");

        assert_eq!(
            ContractPermissionDescriptor::from_stack_value(StackValue::Null).unwrap(),
            ContractPermissionDescriptor::Wildcard
        );
        assert_eq!(
            ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(hash.to_bytes()))
                .unwrap(),
            ContractPermissionDescriptor::Hash(hash)
        );
        assert_eq!(
            ContractPermissionDescriptor::from_stack_value(StackValue::Buffer(group_bytes))
                .unwrap(),
            ContractPermissionDescriptor::Group(group)
        );
        assert_eq!(
            ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(b"*".to_vec()))
                .unwrap(),
            ContractPermissionDescriptor::Wildcard
        );
    }
}

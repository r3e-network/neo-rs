//! ContractPermissionDescriptor - matches C# Neo.SmartContract.Manifest.ContractPermissionDescriptor exactly

use super::contract_group::ContractGroup;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use neo_vm::impl_interoperable_via_stack_value;

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
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        if let Some(s) = json.as_str() {
            match s.len() {
                1 if s == "*" => return Ok(ContractPermissionDescriptor::Wildcard),
                42 => {
                    return UInt160::parse(s)
                        .map(ContractPermissionDescriptor::Hash)
                        .map_err(|e| CoreError::other(e.to_string()));
                }
                66 => {
                    let bytes = hex::decode(s).map_err(|e| CoreError::other(e.to_string()))?;
                    return ECPoint::decode_secp256r1(&bytes)
                        .map(ContractPermissionDescriptor::Group)
                        .map_err(|e| CoreError::other(e.to_string()));
                }
                _ => {}
            }
        }
        Err(CoreError::other("Invalid permission descriptor"))
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
    pub fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        match stack_value {
            StackValue::Null => Ok(Self::create_wildcard()),
            StackValue::ByteString(bytes) | StackValue::Buffer(_, bytes) => {
                Self::from_bytes(&bytes)
            }
            other => Err(CoreError::other(format!(
                "Unsupported stack value type for ContractPermissionDescriptor: {:?}",
                other
            ))),
        }
    }

    /// Creates a descriptor from a stack item encoded form.
    pub fn from_stack_item(item: &StackItem) -> CoreResult<Self> {
        Self::from_stack_value(StackValue::try_from(item.clone()).map_err(|_| {
            CoreError::other(format!(
                "Unsupported stack item type for ContractPermissionDescriptor: {:?}",
                item.stack_item_type()
            ))
        })?)
    }

    /// Builds a descriptor from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        match bytes.len() {
            20 => Ok(Self::create_hash(UInt160::from_bytes(bytes).map_err(
                |e| CoreError::other(format!("Invalid UInt160 bytes: {}", e)),
            )?)),
            33 => Ok(Self::create_group(
                ECPoint::decode_secp256r1(bytes)
                    .map_err(|e| CoreError::other(format!("Invalid ECPoint bytes: {}", e)))?,
            )),
            len => Err(CoreError::other(format!(
                "Invalid descriptor byte length: {}",
                len
            ))),
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

impl_interoperable_via_stack_value!(ContractPermissionDescriptor);

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::{ECPoint, Secp256k1Crypto, Secp256r1Crypto};
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
            ContractPermissionDescriptor::from_stack_value(StackValue::Buffer(0, group_bytes))
                .unwrap(),
            ContractPermissionDescriptor::Group(group)
        );
    }

    #[test]
    fn permission_descriptor_rejects_invalid_stack_byte_lengths_like_csharp() {
        assert!(
            ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(Vec::new()))
                .is_err()
        );
        assert!(
            ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(b"*".to_vec()))
                .is_err()
        );
    }

    #[test]
    fn permission_descriptor_from_json_uses_csharp_lengths_and_curve() {
        let hash = UInt160::from_bytes(&[0x44; 20]).expect("hash");
        assert_eq!(
            ContractPermissionDescriptor::from_json(&serde_json::Value::String(hash.to_string()))
                .unwrap(),
            ContractPermissionDescriptor::Hash(hash)
        );
        assert!(
            ContractPermissionDescriptor::from_json(&serde_json::Value::String(
                hash.to_string().trim_start_matches("0x").to_string()
            ))
            .is_err()
        );

        let group = group_key();
        let compressed = group.encode_point(true).expect("compressed group");
        assert_eq!(
            ContractPermissionDescriptor::from_json(&serde_json::Value::String(hex::encode(
                &compressed
            )))
            .unwrap(),
            ContractPermissionDescriptor::Group(group.clone())
        );

        let uncompressed = group.encode_point(false).expect("uncompressed group");
        assert!(
            ContractPermissionDescriptor::from_json(&serde_json::Value::String(hex::encode(
                uncompressed
            )))
            .is_err()
        );
    }

    #[test]
    fn permission_descriptor_rejects_non_secp256r1_stack_group_like_csharp() {
        let private_key = [2u8; 32];
        let k1_group =
            Secp256k1Crypto::derive_public_key(&private_key).expect("secp256k1 public key");

        assert!(
            ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(
                k1_group.to_vec()
            ))
            .is_err()
        );
    }
}

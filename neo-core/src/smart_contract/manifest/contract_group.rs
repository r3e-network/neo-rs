//!
//! Represents a set of mutually trusted contracts identified by a public key
//! and accompanied by a signature for the contract hash.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::constants::ADDRESS_SIZE;
use crate::smart_contract::interoperable::Interoperable;
use crate::neo_vm::StackItem;
use crate::{ECCurve, ECPoint};
use base64::{engine::general_purpose, Engine as _};
use std::convert::TryFrom;
// Removed neo_cryptography dependency - using external crypto crates directly
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};

/// Represents a set of mutually trusted contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractGroup {
    /// The public key of the group.
    pub pub_key: ECPoint,

    /// The signature of the contract hash which can be verified by the public key.
    pub signature: Vec<u8>,
}

impl ContractGroup {
    /// Creates a new contract group.
    pub fn new(pub_key: ECPoint, signature: Vec<u8>) -> Self {
        Self { pub_key, signature }
    }

    /// Size in bytes when serialized.
    pub fn size(&self) -> usize {
        33 + self.signature.len()
    }

    /// Validates the contract group.
    pub fn validate(&self) -> Result<()> {
        if !self.pub_key.is_valid() {
            return Err(Error::invalid_data("Invalid public key in group"));
        }

        if self.signature.len() != 64 {
            return Err(Error::invalid_data("Invalid signature length in group"));
        }

        Ok(())
    }

    /// Verifies the group signature for a given contract hash.
    pub fn verify_signature(&self, contract_hash: &[u8]) -> Result<bool> {
        if contract_hash.len() != ADDRESS_SIZE {
            return Err(Error::invalid_data("Invalid contract hash length"));
        }

        if self.signature.len() != 64 {
            return Err(Error::invalid_data("Invalid signature length"));
        }

        let public_key_bytes = self
            .pub_key
            .encode_compressed()
            .map_err(|e| Error::invalid_data(format!("Failed to encode public key: {}", e)))?;

        // Convert signature to array format
        let signature_array: [u8; 64] = <[u8; 64]>::try_from(self.signature.as_slice())
            .map_err(|_| Error::invalid_data("Invalid signature length"))?;

        match neo_crypto::crypto_utils::Secp256r1Crypto::verify(
            contract_hash,
            &signature_array,
            &public_key_bytes,
        ) {
            Ok(is_valid) => Ok(is_valid),
            Err(e) => {
                tracing::info!("Error verifying contract group signature: {}", e);
                Ok(false)
            }
        }
    }

    /// Builds a contract group from a neo-vm-rs stack value.
    ///
    /// # Errors
    ///
    /// Returns `Error` if the stack value is not a valid struct with two elements.
    pub fn try_from_stack_value(stack_value: StackValue) -> Result<Self> {
        let items = match stack_value {
            StackValue::Struct(items) => items,
            other => {
                return Err(Error::invalid_data(format!(
                    "ContractGroup expects struct stack value, found {:?}",
                    other.compact_type_tag()
                )));
            }
        };

        if items.len() < 2 {
            return Err(Error::invalid_data(
                "ContractGroup stack value must contain two elements",
            ));
        }

        let pub_key_bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| Error::invalid_data("ContractGroup public key must be byte string"))?;
        let signature_bytes = items[1]
            .to_byte_string_bytes()
            .ok_or_else(|| Error::invalid_data("ContractGroup signature must be byte string"))?;

        let pub_key = ECPoint::from_bytes(&pub_key_bytes)
            .map_err(|e| Error::invalid_data(format!("Failed to decode ECPoint: {}", e)))?;

        Ok(Self {
            pub_key,
            signature: signature_bytes,
        })
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        let pub_key_bytes = self.pub_key.encode_point(true).unwrap_or_else(|e| {
            tracing::error!("Failed to encode ECPoint: {}", e);
            self.pub_key.to_bytes()
        });

        StackValue::Struct(vec![
            StackValue::ByteString(pub_key_bytes),
            StackValue::ByteString(self.signature.clone()),
        ])
    }

    /// Builds a contract group from a VM stack item.
    ///
    /// # Errors
    ///
    /// Returns `Error` if the stack item is not a valid struct with two elements.
    pub fn try_from_stack_item_value(stack_item: &StackItem) -> Result<Self> {
        Self::try_from_stack_value(StackValue::try_from(stack_item.clone()).map_err(|error| {
            Error::invalid_data(format!(
                "Failed to convert ContractGroup StackItem to StackValue: {error}"
            ))
        })?)
    }

    /// Builds a contract group from a VM stack item (panics on error).
    ///
    /// Prefer `try_from_stack_item_value` for fallible construction.
    #[inline]
    pub fn from_stack_item_value(stack_item: &StackItem) -> Self {
        Self::try_from_stack_item_value(stack_item).expect("Invalid ContractGroup stack item")
    }
}

impl Serialize for ContractGroup {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let pubkey_hex = hex::encode(
            self.pub_key
                .encode_compressed()
                .map_err(|e| serde::ser::Error::custom(e.to_string()))?,
        );
        let signature_b64 = general_purpose::STANDARD.encode(&self.signature);

        let helper = ContractGroupSerde {
            pubkey: pubkey_hex,
            signature: signature_b64,
        };

        helper.serialize(serializer)
    }
}

impl Interoperable for ContractGroup {
    fn from_stack_item(&mut self, stack_item: StackItem) -> std::result::Result<(), crate::neo_vm::VmError> {
        match StackValue::try_from(stack_item)
            .map_err(|error| {
                crate::neo_vm::VmError::invalid_operation_msg(format!(
                    "Failed to convert ContractGroup StackItem to StackValue: {error}"
                ))
            })
            .and_then(|sv| Self::try_from_stack_value(sv).map_err(|e| crate::neo_vm::VmError::invalid_operation_msg(e.to_string())))
        {
            Ok(group) => *self = group,
            Err(e) => {
                tracing::error!("Failed to parse ContractGroup from stack item: {}", e);
            }
        }
        Ok(())
    }

    fn to_stack_item(&self) -> std::result::Result<StackItem, crate::neo_vm::VmError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert ContractGroup StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl<'de> Deserialize<'de> for ContractGroup {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = ContractGroupSerde::deserialize(deserializer)?;

        let pubkey_bytes = hex::decode(helper.pubkey)
            .map_err(|e| serde::de::Error::custom(format!("Invalid group pubkey: {}", e)))?;
        let pub_key = ECPoint::decode(&pubkey_bytes, ECCurve::secp256r1())
            .map_err(|e| serde::de::Error::custom(format!("Invalid group pubkey: {}", e)))?;

        let signature = general_purpose::STANDARD
            .decode(helper.signature.as_bytes())
            .map_err(|e| serde::de::Error::custom(format!("Invalid group signature: {}", e)))?;

        Ok(Self { pub_key, signature })
    }
}

#[derive(Serialize, Deserialize)]
struct ContractGroupSerde {
    #[serde(rename = "pubkey")]
    pub pubkey: String,
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_group() -> ContractGroup {
        let encoded =
            hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .expect("hex");
        let pub_key = ECPoint::decode(&encoded, ECCurve::secp256r1()).expect("valid ECPoint");

        ContractGroup::new(pub_key, vec![0xAB; 64])
    }

    #[test]
    fn contract_group_projects_to_neo_vm_rs_stack_value() {
        let group = sample_group();
        let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

        assert_eq!(
            group.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::ByteString(pub_key_bytes),
                StackValue::ByteString(vec![0xAB; 64]),
            ])
        );
    }

    #[test]
    fn contract_group_stack_item_projection_matches_stack_value_projection() {
        let group = sample_group();
        let expected = StackItem::try_from(group.to_stack_value()).unwrap();

        assert_eq!(group.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn contract_group_reads_from_neo_vm_rs_stack_value() {
        let group = sample_group();
        let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

        let decoded = ContractGroup::try_from_stack_value(StackValue::Struct(vec![
            StackValue::ByteString(pub_key_bytes),
            StackValue::ByteString(vec![0xCD; 64]),
        ]))
        .unwrap();

        assert_eq!(decoded.pub_key, group.pub_key);
        assert_eq!(decoded.signature, vec![0xCD; 64]);
    }
}

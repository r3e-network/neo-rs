//!
//! Represents a set of mutually trusted contracts identified by a public key
//! and accompanied by a signature for the contract hash.

use base64::{Engine as _, engine::general_purpose};
use neo_crypto::{ECCurve, ECPoint};
use neo_error::CoreError;
use neo_error::CoreResult;
use neo_primitives::constants::ADDRESS_SIZE;
use neo_primitives::hex_util;
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm::StackItem;
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
    pub fn validate(&self) -> CoreResult<()> {
        if !self.pub_key.is_valid() {
            return Err(CoreError::invalid_data("Invalid public key in group"));
        }

        if self.signature.len() != 64 {
            return Err(CoreError::invalid_data("Invalid signature length in group"));
        }

        Ok(())
    }

    /// Creates from JSON.
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let group: Self =
            serde_json::from_value(json.clone()).map_err(|e| CoreError::other(e.to_string()))?;
        if group.signature.len() != 64 {
            return Err(CoreError::other(format!(
                "Signature length({}) is not 64",
                group.signature.len()
            )));
        }
        Ok(group)
    }

    /// Verifies the group signature for a given contract hash.
    pub fn verify_signature(&self, contract_hash: &[u8]) -> CoreResult<bool> {
        if contract_hash.len() != ADDRESS_SIZE {
            return Err(CoreError::invalid_data("Invalid contract hash length"));
        }

        if self.signature.len() != 64 {
            return Err(CoreError::invalid_data("Invalid signature length"));
        }

        let public_key_bytes = self
            .pub_key
            .encode_compressed()
            .map_err(|e| CoreError::invalid_data(format!("Failed to encode public key: {}", e)))?;

        // Convert signature to array format
        let signature_array: [u8; 64] = <[u8; 64]>::try_from(self.signature.as_slice())
            .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;

        match neo_crypto::Secp256r1Crypto::verify(
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
    pub fn try_from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let items = match stack_value {
            StackValue::Struct(items) => items,
            other => {
                return Err(CoreError::invalid_data(format!(
                    "ContractGroup expects struct stack value, found {:?}",
                    other.compact_type_tag()
                )));
            }
        };

        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "ContractGroup stack value must contain two elements",
            ));
        }

        let pub_key_bytes = items[0].to_byte_string_bytes().ok_or_else(|| {
            CoreError::invalid_data("ContractGroup public key must be byte string")
        })?;
        let signature_bytes = items[1].to_byte_string_bytes().ok_or_else(|| {
            CoreError::invalid_data("ContractGroup signature must be byte string")
        })?;

        let pub_key = ECPoint::from_bytes(&pub_key_bytes)
            .map_err(|e| CoreError::invalid_data(format!("Failed to decode ECPoint: {}", e)))?;

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
    pub fn try_from_stack_item_value(stack_item: &StackItem) -> CoreResult<Self> {
        Self::try_from_stack_value(StackValue::try_from(stack_item.clone()).map_err(|error| {
            CoreError::invalid_data(format!(
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
        let pubkey_bytes = self
            .pub_key
            .encode_compressed()
            .map_err(|e| serde::ser::Error::custom(e.to_string()))?;
        let pubkey_hex = hex_util::encode_hex(&pubkey_bytes);
        let signature_b64 = general_purpose::STANDARD.encode(&self.signature);

        let helper = ContractGroupSerde {
            pubkey: pubkey_hex,
            signature: signature_b64,
        };

        helper.serialize(serializer)
    }
}

impl Interoperable for ContractGroup {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        *self = Self::try_from_stack_value(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))?;
        Ok(())
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
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

        let pubkey_bytes = hex_util::decode_hex(&helper.pubkey)
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
#[path = "../../tests/manifest/contract_group.rs"]
mod tests;

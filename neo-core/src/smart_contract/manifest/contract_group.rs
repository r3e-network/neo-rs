//!
//! Represents a set of mutually trusted contracts identified by a public key
//! and accompanied by a signature for the contract hash.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::neo_config::ADDRESS_SIZE;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::{ECCurve, ECPoint};
use base64::{engine::general_purpose, Engine as _};
use std::convert::TryFrom;
// Removed neo_cryptography dependency - using external crypto crates directly
use neo_vm::StackItem;
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

        match crate::cryptography::crypto_utils::Secp256r1Crypto::verify(
            contract_hash,
            &signature_array,
            &public_key_bytes,
        ) {
            Ok(is_valid) => Ok(is_valid),
            Err(e) => {
                log::info!("Error verifying contract group signature: {}", e);
                Ok(false)
            }
        }
    }

    /// Builds a contract group from a VM stack item.
    ///
    /// # Errors
    ///
    /// Returns `Error` if the stack item is not a valid struct with two elements.
    pub fn try_from_stack_item_value(stack_item: &StackItem) -> Result<Self> {
        let struct_item = match stack_item {
            StackItem::Struct(struct_item) => struct_item,
            other => {
                return Err(Error::invalid_data(format!(
                    "ContractGroup expects struct stack item, found {:?}",
                    other.stack_item_type()
                )));
            }
        };

        let items = struct_item.items();
        if items.len() < 2 {
            return Err(Error::invalid_data(
                "ContractGroup stack item must contain two elements",
            ));
        }

        let pub_key_bytes = items[0]
            .as_bytes()
            .map_err(|_| Error::invalid_data("ContractGroup public key must be byte string"))?;
        let signature_bytes = items[1]
            .as_bytes()
            .map_err(|_| Error::invalid_data("ContractGroup signature must be byte string"))?;

        let pub_key = ECPoint::from_bytes(&pub_key_bytes)
            .map_err(|e| Error::invalid_data(format!("Failed to decode ECPoint: {}", e)))?;

        Ok(Self {
            pub_key,
            signature: signature_bytes,
        })
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

impl IInteroperable for ContractGroup {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        match Self::try_from_stack_item_value(&stack_item) {
            Ok(group) => *self = group,
            Err(e) => {
                tracing::error!("Failed to parse ContractGroup from stack item: {}", e);
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        let pub_key_bytes = self.pub_key.encode_point(true).unwrap_or_else(|e| {
            tracing::error!("Failed to encode ECPoint: {}", e);
            self.pub_key.to_bytes()
        });

        StackItem::from_struct(vec![
            StackItem::from_byte_string(pub_key_bytes),
            StackItem::from_byte_string(self.signature.clone()),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
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

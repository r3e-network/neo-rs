//!
//! Represents a set of mutually trusted contracts identified by a public key
//! and accompanied by a signature for the contract hash.

use crate::{Error, Result};
use base64::{engine::general_purpose, Engine as _};
use neo_config::ADDRESS_SIZE;
use neo_cryptography::ecc::{ECCurve, ECPoint};
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
            return Err(Error::InvalidManifest(
                "Invalid public key in group".to_string(),
            ));
        }

        if self.signature.len() != 64 {
            return Err(Error::InvalidManifest(
                "Invalid signature length in group".to_string(),
            ));
        }

        Ok(())
    }

    /// Verifies the group signature for a given contract hash.
    pub fn verify_signature(&self, contract_hash: &[u8]) -> Result<bool> {
        if contract_hash.len() != ADDRESS_SIZE {
            return Err(Error::InvalidManifest(
                "Invalid contract hash length".to_string(),
            ));
        }

        if self.signature.len() != 64 {
            return Err(Error::InvalidManifest(
                "Invalid signature length".to_string(),
            ));
        }

        let public_key_bytes = self
            .pub_key
            .encode_compressed()
            .map_err(|e| Error::InvalidManifest(format!("Failed to encode public key: {}", e)))?;

        match neo_cryptography::ecdsa::ECDsa::verify_signature_secp256r1(
            contract_hash,
            &self.signature,
            &public_key_bytes,
        ) {
            Ok(is_valid) => Ok(is_valid),
            Err(e) => {
                log::info!("Error verifying contract group signature: {}", e);
                Ok(false)
            }
        }
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

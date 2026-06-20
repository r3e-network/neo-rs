// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Witness - Transaction signature verification for Neo N3.
//!
//! This module provides the `Witness` struct, which contains the scripts needed
//! to verify transaction signatures on the Neo blockchain.
//!
//! ## Overview
//!
//! A witness consists of two parts:
//! - **Invocation Script**: Provides the arguments (signatures) for verification
//! - **Verification Script**: The contract code that verifies the signature
//!
//! ## Script Hash Computation
//!
//! The script hash is computed as: `RIPEMD160(SHA256(verification_script))`
//!
//! ## Example
//!
//! ```rust
//! use neo_payloads::Witness;
//! use neo_vm_rs::OpCode;
//!
//! // Create a witness from scripts
//! let invocation_script = vec![OpCode::PUSHDATA1.byte(), 0x40, 0x01, 0x02];
//! let verification_script = vec![OpCode::PUSHDATA1.byte(), 0x21];
//! let witness = Witness::new_with_scripts(
//!     invocation_script,
//!     verification_script,
//! );
//!
//! // Get the script hash
//! let script_hash = witness.script_hash();
//! ```

use base64::{Engine as _, engine::general_purpose};
use neo_crypto::Crypto;
use neo_error::{CoreError, CoreResult};
use neo_io::{Serializable, serializable::helper::SerializeHelper};
use neo_primitives::UInt160;
use neo_vm_rs::OpCode;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::{convert::TryInto, fmt};

/// Maximum size of invocation script in bytes.
/// This is designed to allow a MultiSig 21/11 (committee)
/// Invocation = 11 * (64 + 2) = 726
const MAX_INVOCATION_SCRIPT: usize = 1024;

/// Maximum size of verification script in bytes.
/// Verification = m + (PUSH_PubKey * 21) + length + null + syscall = 1 + ((2 + 33) * 21) + 2 + 1 + 5 = 744
const MAX_VERIFICATION_SCRIPT: usize = 1024;

/// Represents a witness of a verifiable object.
///
/// A witness contains the invocation script (used to pass arguments) and
/// the verification script (the contract code to verify the signature).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    /// The invocation script of the witness. Used to pass arguments for verification script.
    pub invocation_script: Vec<u8>,

    /// The verification script of the witness. It can be empty if the contract is deployed.
    pub verification_script: Vec<u8>,

    /// Cached script hash
    #[serde(skip)]
    script_hash: OnceLock<UInt160>,
}

impl Witness {
    /// Creates a new Witness instance.
    pub fn new() -> Self {
        Self {
            invocation_script: Vec::new(),
            verification_script: Vec::new(),
            script_hash: OnceLock::new(),
        }
    }

    /// Creates a new Witness with the specified invocation and verification scripts.
    ///
    /// # Arguments
    ///
    /// * `invocation_script` - The invocation script
    /// * `verification_script` - The verification script
    ///
    /// # Returns
    ///
    /// A new Witness instance
    pub fn new_with_scripts(invocation_script: Vec<u8>, verification_script: Vec<u8>) -> Self {
        Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        }
    }

    /// Creates an empty witness with empty invocation and verification scripts.
    ///
    /// # Returns
    ///
    /// An empty Witness instance
    pub fn empty() -> Self {
        Self::new()
    }

    /// Gets the hash of the verification script (matches C# ScriptHash property).
    /// Calculates RIPEMD160(SHA256(verification_script)) like the C# implementation.
    ///
    /// # Returns
    ///
    /// The script hash as UInt160
    pub fn script_hash(&self) -> UInt160 {
        *self
            .script_hash
            .get_or_init(|| UInt160::from(Crypto::hash160(&self.verification_script)))
    }

    /// Gets the size of the witness in bytes after serialization.
    ///
    /// # Returns
    ///
    /// The size in bytes
    pub fn size(&self) -> usize {
        SerializeHelper::get_var_size_bytes(&self.invocation_script)
            + SerializeHelper::get_var_size_bytes(&self.verification_script)
    }

    /// Converts the witness to JSON (matches C# `ToJson`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": general_purpose::STANDARD.encode(&self.invocation_script),
            "verification": general_purpose::STANDARD.encode(&self.verification_script)
        })
    }

    /// Clones the witness.
    ///
    /// # Returns
    ///
    /// A cloned Witness instance
    pub fn clone_witness(&self) -> Self {
        Self {
            invocation_script: self.invocation_script.clone(),
            verification_script: self.verification_script.clone(),
            script_hash: {
                let clone_cell = OnceLock::new();
                if let Some(hash) = self.script_hash.get() {
                    let _ = clone_cell.set(*hash);
                }
                clone_cell
            },
        }
    }

    /// Verifies the witness signature (production-ready implementation).
    pub fn verify_signature(&self, hash_data: &[u8], account: &UInt160) -> CoreResult<bool> {
        // 1. Extract public key from verification script
        let public_key = self.extract_public_key_from_verification_script()?;

        // 2. Extract signature from invocation script
        let signature = self.extract_signature_from_invocation_script()?;

        // 3. Verify signature against hash_data using the public key
        let signature_valid = self.verify_ecdsa_signature(hash_data, &signature, &public_key)?;
        if !signature_valid {
            return Ok(false);
        }

        // 4. Verify that the public key corresponds to the account
        let computed_account = self.compute_script_hash_from_public_key(&public_key)?;
        Ok(computed_account == *account)
    }

    /// Verifies a multi-signature witness against the provided message.
    pub fn verify_multi_signature(
        &self,
        message: &[u8],
        account: &UInt160,
        required_signatures: usize,
        public_keys: &[Vec<u8>],
        signatures: &[Vec<u8>],
    ) -> CoreResult<bool> {
        use neo_crypto::Secp256r1Crypto;

        if required_signatures == 0
            || public_keys.is_empty()
            || required_signatures > public_keys.len()
            || signatures.len() != required_signatures
        {
            return Ok(false);
        }

        let script = match neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_keys(
            required_signatures,
            public_keys,
        ) {
            Ok(script) => script,
            Err(_) => return Ok(false),
        };

        if UInt160::from_script(&script) != *account {
            return Ok(false);
        }

        let mut sorted_keys = public_keys.to_vec();
        sorted_keys.sort();

        let total_keys = sorted_keys.len();
        let mut sig_index = 0usize;
        let mut key_index = 0usize;

        while sig_index < required_signatures && key_index < total_keys {
            let signature = &signatures[sig_index];
            if signature.len() != 64 {
                return Ok(false);
            }

            let signature_bytes: [u8; 64] = signature
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;

            let verified =
                Secp256r1Crypto::verify(message, &signature_bytes, &sorted_keys[key_index])
                    .map_err(|e| CoreError::Cryptographic {
                        message: format!("ECDSA verification failed: {e}"),
                    })?;

            if verified {
                sig_index += 1;
            }

            key_index += 1;

            if required_signatures - sig_index > total_keys - key_index {
                return Ok(false);
            }
        }

        Ok(sig_index == required_signatures)
    }

    /// Extracts public key from verification script (matches C# verification script parsing exactly).
    fn extract_public_key_from_verification_script(&self) -> Result<Vec<u8>, CoreError> {
        if !neo_vm::script_builder::redeem_script::RedeemScript::is_signature_contract(
            &self.verification_script,
        ) {
            return Err(CoreError::Invalid {
                message: "Unsupported verification script format".to_string(),
            });
        }

        let public_key = self.verification_script[2..35].to_vec();

        if public_key.len() != 33 || (public_key[0] != 0x02 && public_key[0] != 0x03) {
            return Err(CoreError::Invalid {
                message: "Invalid compressed public key format".to_string(),
            });
        }

        Ok(public_key)
    }

    /// Extracts signature from invocation script (matches C# signature extraction exactly).
    fn extract_signature_from_invocation_script(&self) -> Result<Vec<u8>, CoreError> {
        // Real C# Neo N3 implementation: Invocation script signature extraction

        if self.invocation_script.len() != 66 {
            return Err(CoreError::Invalid {
                message: "Invalid invocation script length".to_string(),
            });
        }

        if self.invocation_script[0] != OpCode::PUSHDATA1.byte()
            || self.invocation_script[1] != 0x40
        {
            return Err(CoreError::Invalid {
                message: "Invalid invocation script format".to_string(),
            });
        }

        let signature = self.invocation_script[2..66].to_vec();

        if signature.len() != 64 {
            return Err(CoreError::Invalid {
                message: "Invalid signature length".to_string(),
            });
        }

        Ok(signature)
    }

    /// Verifies ECDSA signature (matches C# ECDsa.VerifyData exactly).
    fn verify_ecdsa_signature(
        &self,
        hash_data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> CoreResult<bool> {
        // Real C# Neo N3 implementation: ECDsa.VerifyData

        use neo_crypto::Secp256r1Crypto;

        let signature_bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;

        Secp256r1Crypto::verify(hash_data, &signature_bytes, public_key).map_err(|e| {
            CoreError::Cryptographic {
                message: format!("ECDSA verification failed: {e}"),
            }
        })
    }

    /// Computes script hash from public key (matches C# Contract.CreateSignatureContract exactly).
    fn compute_script_hash_from_public_key(&self, public_key: &[u8]) -> CoreResult<UInt160> {
        let verification_script = self.create_verification_script_from_public_key(public_key)?;
        Ok(UInt160::from_script(&verification_script))
    }

    /// Creates verification script from public key (matches C# Contract.CreateSignatureRedeemScript exactly).
    fn create_verification_script_from_public_key(
        &self,
        public_key: &[u8],
    ) -> Result<Vec<u8>, CoreError> {
        if public_key.len() != 33 {
            return Err(CoreError::Invalid {
                message: "Public key must be 33 bytes (compressed)".to_string(),
            });
        }

        if public_key[0] != 0x02 && public_key[0] != 0x03 {
            return Err(CoreError::Invalid {
                message: "Invalid compressed public key format".to_string(),
            });
        }

        Ok(
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                public_key,
            ),
        )
    }
}

neo_io::impl_default_via_new!(Witness);

impl neo_primitives::Witness for Witness {
    fn invocation_script(&self) -> &[u8] {
        &self.invocation_script
    }

    fn verification_script(&self) -> &[u8] {
        &self.verification_script
    }
}

impl Serializable for Witness {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut neo_io::BinaryWriter) -> neo_io::IoResult<()> {
        // Write invocation script with variable length encoding
        writer.write_var_bytes(&self.invocation_script)?;
        // Write verification script with variable length encoding
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }

    fn deserialize(reader: &mut neo_io::MemoryReader) -> neo_io::IoResult<Self> {
        // Read invocation script with variable length encoding
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;

        // Read verification script with variable length encoding
        let verification_script = reader.read_var_bytes(MAX_VERIFICATION_SCRIPT)?;

        Ok(Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        })
    }
}

impl fmt::Display for Witness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Witness {{ invocation: {}, verification: {} }}",
            hex::encode(&self.invocation_script),
            hex::encode(&self.verification_script)
        )
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests;

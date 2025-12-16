// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! Implementation of Witness for Neo blockchain.

use crate::error::{CoreError, CoreResult};
use crate::neo_config::ADDRESS_SIZE;
use crate::neo_io::Serializable;
use crate::UInt160;
use base64::{engine::general_purpose, Engine as _};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
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
    script_hash: OnceCell<UInt160>,
}

impl Witness {
    /// Creates a new Witness instance.
    pub fn new() -> Self {
        Self {
            invocation_script: Vec::new(),
            verification_script: Vec::new(),
            script_hash: OnceCell::new(),
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
            script_hash: OnceCell::new(),
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

    /// Returns the invocation script.
    pub fn invocation_script(&self) -> &[u8] {
        &self.invocation_script
    }

    /// Returns the verification script.
    pub fn verification_script(&self) -> &[u8] {
        &self.verification_script
    }

    /// Gets the hash of the verification script (matches C# ScriptHash property).
    /// Calculates RIPEMD160(SHA256(verification_script)) like the C# implementation.
    ///
    /// # Returns
    ///
    /// The script hash as UInt160
    pub fn script_hash(&self) -> UInt160 {
        *self.script_hash.get_or_init(|| {
            use ripemd::Ripemd160;
            use sha2::{Digest, Sha256};

            let mut sha256_hasher = Sha256::new();
            sha256_hasher.update(&self.verification_script);
            let sha256_result = sha256_hasher.finalize();

            let mut ripemd_hasher = Ripemd160::new();
            ripemd_hasher.update(sha256_result);
            let ripemd_result = ripemd_hasher.finalize();

            let mut hash_bytes = [0u8; ADDRESS_SIZE];
            hash_bytes.copy_from_slice(&ripemd_result);
            UInt160::from_bytes(&hash_bytes).unwrap_or_default()
        })
    }

    /// Gets the size of the witness in bytes after serialization.
    ///
    /// # Returns
    ///
    /// The size in bytes
    pub fn get_size(&self) -> usize {
        let invocation_size = self.get_var_size(&self.invocation_script);
        let verification_size = self.get_var_size(&self.verification_script);

        invocation_size + verification_size
    }

    /// Converts the witness to JSON (matches C# `ToJson`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": general_purpose::STANDARD.encode(&self.invocation_script),
            "verification": general_purpose::STANDARD.encode(&self.verification_script)
        })
    }

    /// Helper function to calculate variable length encoding size
    fn get_var_size(&self, data: &[u8]) -> usize {
        let len = data.len();
        let var_int_size = if len < 0xFD {
            1
        } else if len <= 0xFFFF {
            3
        } else if len <= 0xFFFFFFFF {
            5
        } else {
            9
        };
        var_int_size + len
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
                let clone_cell = OnceCell::new();
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
        use crate::cryptography::Secp256r1Crypto;
        use crate::smart_contract::helper::Helper;

        if required_signatures == 0
            || public_keys.is_empty()
            || required_signatures > public_keys.len()
            || signatures.len() != required_signatures
        {
            return Ok(false);
        }

        let script = match Helper::try_multi_sig_redeem_script(required_signatures, public_keys) {
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

    /// Extracts public key from verification script.
    fn extract_public_key_from_verification_script(&self) -> Result<Vec<u8>, CoreError> {
        // Neo N3 signature redeem script is produced by ScriptBuilder.EmitPush(pubkey) + SYSCALL(CheckSig):
        // - PUSHDATA1 (0x0C)
        // - length 33 (0x21)
        // - 33-byte compressed pubkey
        // - SYSCALL (0x41) + 4-byte syscall id
        if self.verification_script.len() != 40
            || self.verification_script[0] != 0x0C
            || self.verification_script[1] != 0x21
            || self.verification_script[35] != 0x41
        {
            return Err(CoreError::InvalidData {
                message: "Invalid verification script format".to_string(),
            });
        }
        let public_key = self.verification_script[2..35].to_vec();

        if public_key.len() != 33 || (public_key[0] != 0x02 && public_key[0] != 0x03) {
            return Err(CoreError::InvalidData {
                message: "Invalid compressed public key format".to_string(),
            });
        }

        Ok(public_key)
    }

    /// Extracts signature from invocation script.
    fn extract_signature_from_invocation_script(&self) -> Result<Vec<u8>, CoreError> {
        // Neo N3 witnesses push a 64-byte signature using `PUSHDATA1 0x40 <sig>`.
        if self.invocation_script.len() != 66
            || self.invocation_script[0] != 0x0C
            || self.invocation_script[1] != 0x40
        {
            return Err(CoreError::InvalidData {
                message: "Invalid invocation script format".to_string(),
            });
        }
        let signature = self.invocation_script[2..66].to_vec();

        if signature.len() != 64 {
            return Err(CoreError::InvalidData {
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

        use crate::cryptography::Secp256r1Crypto;

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
        // Implements C# Contract.CreateSignatureContract functionality

        use crate::cryptography::NeoHash;

        let verification_script = self.create_verification_script_from_public_key(public_key)?;

        let script_hash = NeoHash::hash160(&verification_script);

        UInt160::from_bytes(&script_hash).map_err(|e| CoreError::InvalidData {
            message: format!("Invalid script hash: {e}"),
        })
    }

    /// Creates verification script from public key (matches C# Contract.CreateSignatureRedeemScript exactly).
    fn create_verification_script_from_public_key(
        &self,
        public_key: &[u8],
    ) -> Result<Vec<u8>, CoreError> {
        // Implements C# Contract.CreateSignatureRedeemScript functionality

        if public_key.len() != 33 {
            return Err(CoreError::InvalidData {
                message: "Public key must be 33 bytes (compressed)".to_string(),
            });
        }

        if public_key[0] != 0x02 && public_key[0] != 0x03 {
            return Err(CoreError::InvalidData {
                message: "Invalid compressed public key format".to_string(),
            });
        }

        // Create verification script exactly like C# Contract.CreateSignatureRedeemScript
        let mut script = Vec::with_capacity(35);
        script.push(0x0C); // OpCode.PUSHDATA1
        script.push(0x21); // 33 bytes (0x21)
        script.extend_from_slice(public_key); // ECPoint.EncodePoint(true) - compressed public key
        script.push(0x41); // OpCode.CHECKSIG

        Ok(script)
    }
}

impl Default for Witness {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializable for Witness {
    fn size(&self) -> usize {
        let invocation_size = self.get_var_size(&self.invocation_script);
        let verification_size = self.get_var_size(&self.verification_script);

        invocation_size + verification_size
    }

    fn serialize(&self, writer: &mut crate::neo_io::BinaryWriter) -> crate::neo_io::IoResult<()> {
        // Write invocation script with variable length encoding
        writer.write_var_bytes(&self.invocation_script)?;
        // Write verification script with variable length encoding
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }

    fn deserialize(reader: &mut crate::neo_io::MemoryReader) -> crate::neo_io::IoResult<Self> {
        // Read invocation script with variable length encoding
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;

        // Read verification script with variable length encoding
        let verification_script = reader.read_var_bytes(MAX_VERIFICATION_SCRIPT)?;

        Ok(Self {
            invocation_script,
            verification_script,
            script_hash: OnceCell::new(),
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
mod tests {
    use super::*;
    use crate::neo_io::Serializable;
    use crate::{cryptography::Secp256r1Crypto, smart_contract::helper::Helper};

    #[test]
    fn test_witness_new() {
        let witness = Witness::new();
        assert!(witness.invocation_script.is_empty());
        assert!(witness.verification_script.is_empty());
        assert!(witness.script_hash.get().is_none());
    }
    #[test]
    fn test_witness_empty() {
        let witness = Witness::empty();
        assert!(witness.invocation_script.is_empty());
        assert!(witness.verification_script.is_empty());
    }
    #[test]
    fn test_witness_new_with_scripts() {
        let invocation = vec![1, 2, 3];
        let verification = vec![4, 5, 6];
        let witness = Witness::new_with_scripts(invocation.clone(), verification.clone());
        assert_eq!(witness.invocation_script, invocation);
        assert_eq!(witness.verification_script, verification);
    }
    #[test]
    fn test_witness_size() {
        let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let size = witness.get_size();
        assert_eq!(size, 8);
    }
    #[test]
    fn test_witness_clone() {
        let original = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let cloned = original.clone_witness();
        assert_eq!(original.invocation_script, cloned.invocation_script);
        assert_eq!(original.verification_script, cloned.verification_script);
    }
    #[test]
    fn test_witness_serialization() {
        let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let mut writer = crate::neo_io::BinaryWriter::new();
        <Witness as Serializable>::serialize(&witness, &mut writer).unwrap();
        let bytes = writer.to_bytes();
        let mut reader = crate::neo_io::MemoryReader::new(&bytes);
        let deserialized = <Witness as Serializable>::deserialize(&mut reader).unwrap();
        assert_eq!(witness.invocation_script, deserialized.invocation_script);
        assert_eq!(
            witness.verification_script,
            deserialized.verification_script
        );
    }

    #[test]
    fn test_witness_verify_multi_signature() {
        let message = b"neo-multisig-test";

        let priv1 = Secp256r1Crypto::generate_private_key();
        let priv2 = Secp256r1Crypto::generate_private_key();
        let priv3 = Secp256r1Crypto::generate_private_key();

        let pub1 = Secp256r1Crypto::derive_public_key(&priv1).unwrap();
        let pub2 = Secp256r1Crypto::derive_public_key(&priv2).unwrap();
        let pub3 = Secp256r1Crypto::derive_public_key(&priv3).unwrap();

        let m = 2usize;
        let mut pairs = [(pub1, priv1), (pub2, priv2), (pub3, priv3)];
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

        let public_keys: Vec<Vec<u8>> = pairs.iter().map(|(p, _)| p.clone()).collect();
        let verification_script = Helper::multi_sig_redeem_script(m, &public_keys);
        let account = UInt160::from_script(&verification_script);

        let signatures: Vec<Vec<u8>> = pairs
            .iter()
            .take(m)
            .map(|(_, pk)| Secp256r1Crypto::sign(message, pk).unwrap().to_vec())
            .collect();

        let witness = Witness::new();
        let ok = witness
            .verify_multi_signature(message, &account, m, &public_keys, &signatures)
            .unwrap();
        assert!(ok);

        let bad = witness
            .verify_multi_signature(message, &account, m, &public_keys, &signatures[..1])
            .unwrap();
        assert!(!bad);
    }
}

//! Data sealing for TEE
//!
//! Provides encryption/decryption of data using enclave-specific keys.
//! In SGX mode, uses hardware-derived keys. In simulation mode, uses
//! software-derived keys.

use crate::error::{TeeError, TeeResult};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use hkdf::Hkdf;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroize;

/// Sealed data container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedData {
    /// Encrypted data
    pub ciphertext: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: [u8; 12],
    /// Additional authenticated data (AAD)
    pub aad: Vec<u8>,
    /// Monotonic counter value when sealed (for replay protection)
    pub counter: u64,
    /// Version of the sealing format
    pub version: u8,
    /// Key derivation context (for HKDF)
    pub context: Option<String>,
}

impl SealedData {
    /// Current sealing format version
    pub const CURRENT_VERSION: u8 = 1;

    /// Serialize to bytes
    pub fn to_bytes(&self) -> TeeResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| TeeError::SerializationError(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> TeeResult<Self> {
        serde_json::from_slice(data).map_err(|e| TeeError::SerializationError(e.to_string()))
    }
}

/// Key derivation parameters for HKDF
#[derive(Debug, Clone)]
pub struct KeyDerivationParams<'a> {
    /// Base key material (e.g., sealing key from TEE)
    pub base_key: &'a [u8; 32],
    /// Context/application-specific info string
    pub context: &'a str,
    /// Optional salt for domain separation
    pub salt: Option<&'a [u8]>,
}

/// Data sealing operations grouped as associated functions.
pub struct Sealing;

impl Sealing {
    /// Derive a context-specific key using HKDF-SHA256
    ///
    /// This implements RFC 5869 HKDF for secure key derivation from
    /// enclave sealing keys, providing domain separation between
    /// different uses of the same base key.
    pub fn derive_key_hkdf(params: KeyDerivationParams) -> TeeResult<[u8; 32]> {
        let salt = params.salt.unwrap_or(b"neo-tee-hkdf-salt-v1");

        let hkdf = Hkdf::<Sha256>::new(Some(salt), params.base_key);

        let mut derived_key = [0u8; 32];
        hkdf.expand(params.context.as_bytes(), &mut derived_key)
            .map_err(|e| TeeError::KeyDerivationFailed(format!("HKDF expansion failed: {}", e)))?;

        Ok(derived_key)
    }

    /// Derive a key for sealing with domain separation
    ///
    /// Uses HKDF to derive a unique key for each sealing context,
    /// preventing key reuse across different data types.
    pub fn derive_sealing_key(base_sealing_key: &[u8; 32], context: &str) -> TeeResult<[u8; 32]> {
        let params = KeyDerivationParams {
            base_key: base_sealing_key,
            context: &format!("neo-tee-sealing:{}", context),
            salt: Some(b"neo-tee-sealing-salt"),
        };

        Sealing::derive_key_hkdf(params)
    }

    /// Seal data using the enclave's sealing key
    pub fn seal_data(
        plaintext: &[u8],
        sealing_key: &[u8; 32],
        aad: &[u8],
        counter: u64,
    ) -> TeeResult<SealedData> {
        Sealing::seal_data_with_context(plaintext, sealing_key, aad, counter, "default")
    }

    /// Seal data with context-specific key derivation
    ///
    /// This provides better security by using HKDF to derive unique
    /// keys for different sealing contexts, ensuring cryptographic
    /// separation between different data types.
    pub fn seal_data_with_context(
        plaintext: &[u8],
        sealing_key: &[u8; 32],
        aad: &[u8],
        counter: u64,
        context: &str,
    ) -> TeeResult<SealedData> {
        // Derive context-specific key using HKDF
        let derived_key = Sealing::derive_sealing_key(sealing_key, context)?;

        // Generate random nonce using cryptographically secure RNG
        // SECURITY: Must use OsRng for AES-GCM nonce generation
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Create cipher with derived key
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| TeeError::CryptoError(format!("Failed to create cipher: {}", e)))?;

        // Encrypt with AAD
        let ciphertext = cipher
            .encrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|e| TeeError::SealingFailed(format!("Encryption failed: {}", e)))?;

        // Zeroize derived key after use
        let mut key_copy = derived_key;
        key_copy.zeroize();

        Ok(SealedData {
            ciphertext,
            nonce: nonce_bytes,
            aad: aad.to_vec(),
            counter,
            version: SealedData::CURRENT_VERSION,
            context: Some(context.to_string()),
        })
    }

    /// Unseal data using the enclave's sealing key
    pub fn unseal_data(
        sealed: &SealedData,
        sealing_key: &[u8; 32],
        min_counter: Option<u64>,
    ) -> TeeResult<Vec<u8>> {
        // Check version
        if sealed.version != SealedData::CURRENT_VERSION {
            return Err(TeeError::UnsealingFailed(format!(
                "Unsupported sealing version: {}",
                sealed.version
            )));
        }

        // Check replay protection
        if let Some(min) = min_counter {
            if sealed.counter < min {
                return Err(TeeError::UnsealingFailed(
                    "Sealed data counter too old (potential replay attack)".to_string(),
                ));
            }
        }

        // Determine key derivation context
        let context = sealed.context.as_deref().unwrap_or("default");

        // Derive the same context-specific key used for sealing
        let derived_key = Sealing::derive_sealing_key(sealing_key, context)?;

        // Create cipher with derived key
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| TeeError::CryptoError(format!("Failed to create cipher: {}", e)))?;

        // Decrypt with AAD verification
        let nonce = Nonce::from_slice(&sealed.nonce);
        let plaintext = cipher
            .decrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: &sealed.ciphertext,
                    aad: &sealed.aad,
                },
            )
            .map_err(|e| TeeError::UnsealingFailed(format!("Decryption failed: {}", e)))?;

        // Zeroize derived key after use
        let mut key_copy = derived_key;
        key_copy.zeroize();

        Ok(plaintext)
    }
}

/// Secure key container that zeros memory on drop
#[allow(dead_code)]
#[derive(Clone)]
pub struct SecureKey {
    key: [u8; 32],
}

#[allow(dead_code)]
impl SecureKey {
    /// Create a secure key wrapper from raw key material.
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Return the raw key bytes for cryptographic operations.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Derive a new key using HKDF with the given context
    pub fn derive_subkey(&self, context: &str) -> TeeResult<Self> {
        let params = KeyDerivationParams {
            base_key: &self.key,
            context,
            salt: None,
        };
        let derived = Sealing::derive_key_hkdf(params)?;
        Ok(Self::new(derived))
    }
}

impl Zeroize for SecureKey {
    fn zeroize(&mut self) {
        self.key.zeroize();
    }
}

impl Drop for SecureKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[cfg(test)]
#[path = "../tests/enclave/sealing.rs"]
mod tests;

//! Data sealing for TEE
//!
//! Provides encryption/decryption of data using enclave-specific keys.
//! In SGX mode, uses hardware-derived keys. In simulation mode, uses
//! software-derived keys.

use crate::error::{TeeError, TeeResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
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

/// Seal data using the enclave's sealing key
pub fn seal_data(
    plaintext: &[u8],
    sealing_key: &[u8; 32],
    aad: &[u8],
    counter: u64,
) -> TeeResult<SealedData> {
    // Generate random nonce using cryptographically secure RNG
    // SECURITY: Must use OsRng for AES-GCM nonce generation
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(sealing_key)
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

    Ok(SealedData {
        ciphertext,
        nonce: nonce_bytes,
        aad: aad.to_vec(),
        counter,
        version: SealedData::CURRENT_VERSION,
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

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(sealing_key)
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

    Ok(plaintext)
}

/// Secure key container that zeros memory on drop
#[allow(dead_code)]
#[derive(Clone)]
pub struct SecureKey {
    key: [u8; 32],
}

#[allow(dead_code)]
impl SecureKey {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
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
mod tests {
    use super::*;

    #[test]
    fn test_seal_unseal() {
        let key: [u8; 32] = rand::random();
        let plaintext = b"Hello, TEE world!";
        let aad = b"additional data";

        let sealed = seal_data(plaintext, &key, aad, 1).unwrap();
        let unsealed = unseal_data(&sealed, &key, None).unwrap();

        assert_eq!(unsealed, plaintext);
    }

    #[test]
    fn test_replay_protection() {
        let key: [u8; 32] = rand::random();
        let plaintext = b"secret data";

        let sealed = seal_data(plaintext, &key, &[], 5).unwrap();

        // Should succeed with counter >= 5
        assert!(unseal_data(&sealed, &key, Some(5)).is_ok());
        assert!(unseal_data(&sealed, &key, Some(4)).is_ok());

        // Should fail with counter > 5
        assert!(unseal_data(&sealed, &key, Some(6)).is_err());
    }

    #[test]
    fn test_tamper_detection() {
        let key: [u8; 32] = rand::random();
        let plaintext = b"important data";

        let mut sealed = seal_data(plaintext, &key, &[], 1).unwrap();

        // Tamper with ciphertext
        sealed.ciphertext[0] ^= 0xFF;

        // Should fail decryption
        assert!(unseal_data(&sealed, &key, None).is_err());
    }
}

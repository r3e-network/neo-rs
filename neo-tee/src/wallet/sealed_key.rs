//! Sealed key storage for TEE wallet

use crate::enclave::{SealedData, Sealing, TeeEnclave};
use crate::error::{TeeError, TeeResult};
use neo_crypto::base58;
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroizing;

/// A sealed private key stored in TEE-protected format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedKey {
    /// The sealed key data
    pub sealed_data: SealedData,
    /// Public key (not sealed, for identification)
    pub public_key: Vec<u8>,
    /// Optional label/name for the key
    pub label: Option<String>,
    /// Script hash derived from the public key
    pub script_hash: [u8; 20],
    /// Creation timestamp
    pub created_at: u64,
}

impl SealedKey {
    /// Create a new sealed key from a private key
    pub fn seal(
        enclave: &TeeEnclave,
        private_key: &[u8],
        public_key: &[u8],
        script_hash: &[u8; 20],
        label: Option<String>,
    ) -> TeeResult<Self> {
        let sealing_key = enclave.sealing_key()?;
        let counter = enclave.increment_counter()?;

        // AAD includes public key and script hash for binding
        let mut aad = Vec::new();
        aad.extend_from_slice(public_key);
        aad.extend_from_slice(script_hash);

        let sealed_data = Sealing::seal_data(private_key, &sealing_key, &aad, counter)?;

        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Self {
            sealed_data,
            public_key: public_key.to_vec(),
            label,
            script_hash: *script_hash,
            created_at,
        })
    }

    /// Unseal the private key.
    ///
    /// The returned bytes are wrapped in [`Zeroizing`] so the private key
    /// material is automatically zeroed when dropped.
    pub fn unseal(&self, enclave: &TeeEnclave) -> TeeResult<Zeroizing<Vec<u8>>> {
        let sealing_key = enclave.sealing_key()?;

        // Verify AAD matches
        let mut expected_aad = Vec::new();
        expected_aad.extend_from_slice(&self.public_key);
        expected_aad.extend_from_slice(&self.script_hash);

        if self.sealed_data.aad != expected_aad {
            return Err(TeeError::UnsealingFailed(
                "AAD mismatch - possible tampering".to_string(),
            ));
        }

        Ok(Zeroizing::new(Sealing::unseal_data(
            &self.sealed_data,
            &sealing_key,
            None,
        )?))
    }

    /// Save sealed key to file
    pub fn save_to_file(&self, path: &Path) -> TeeResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load sealed key from file
    pub fn load_from_file(path: &Path) -> TeeResult<Self> {
        let json = std::fs::read_to_string(path)?;
        let sealed: Self = serde_json::from_str(&json)?;
        Ok(sealed)
    }

    /// Get the Neo address string
    pub fn address(&self) -> String {
        // Convert script hash to Neo address format
        // Address = Base58Check(0x35 || script_hash)
        let mut data = vec![0x35u8]; // Neo N3 address version
        data.extend_from_slice(&self.script_hash);
        base58::encode_check(&data)
    }
}

#[cfg(test)]
#[path = "../tests/wallet/sealed_key.rs"]
mod tests;

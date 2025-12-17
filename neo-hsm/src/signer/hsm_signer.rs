//! HSM Signer trait definition

use crate::device::HsmDeviceInfo;
use crate::error::HsmResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Information about a key stored in the HSM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmKeyInfo {
    /// Key identifier (device-specific)
    pub key_id: String,

    /// Public key (compressed, 33 bytes for secp256r1)
    pub public_key: Vec<u8>,

    /// Neo script hash derived from public key (20 bytes)
    pub script_hash: [u8; 20],

    /// Human-readable label
    pub label: Option<String>,

    /// Derivation path (for HD wallets like Ledger)
    pub derivation_path: Option<String>,

    /// Key algorithm (e.g., "secp256r1", "secp256k1")
    pub algorithm: String,
}

impl HsmKeyInfo {
    /// Create a new HsmKeyInfo
    pub fn new(
        key_id: impl Into<String>,
        public_key: Vec<u8>,
        script_hash: [u8; 20],
    ) -> Self {
        Self {
            key_id: key_id.into(),
            public_key,
            script_hash,
            label: None,
            derivation_path: None,
            algorithm: "secp256r1".to_string(),
        }
    }

    /// Set the label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the derivation path
    pub fn with_derivation_path(mut self, path: impl Into<String>) -> Self {
        self.derivation_path = Some(path.into());
        self
    }

    /// Get the Neo address (Base58Check encoded)
    pub fn neo_address(&self) -> String {
        // Neo N3 address version byte
        const ADDRESS_VERSION: u8 = 0x35;

        let mut data = vec![ADDRESS_VERSION];
        data.extend_from_slice(&self.script_hash);

        // Double SHA256 for checksum
        let hash1 = neo_crypto::Crypto::sha256(&data);
        let hash2 = neo_crypto::Crypto::sha256(&hash1);
        let checksum = &hash2[..4];

        data.extend_from_slice(checksum);
        bs58::encode(data).into_string()
    }
}

/// Common interface for all HSM implementations
#[async_trait]
pub trait HsmSigner: Send + Sync {
    /// Get device information
    fn device_info(&self) -> &HsmDeviceInfo;

    /// Check if the HSM is ready for operations
    fn is_ready(&self) -> bool;

    /// Unlock the HSM with PIN (if required)
    async fn unlock(&self, pin: &str) -> HsmResult<()>;

    /// Lock the HSM
    fn lock(&self);

    /// Check if HSM is currently locked
    fn is_locked(&self) -> bool;

    /// List all available keys
    async fn list_keys(&self) -> HsmResult<Vec<HsmKeyInfo>>;

    /// Get a specific key by ID or derivation path
    async fn get_key(&self, key_id: &str) -> HsmResult<HsmKeyInfo>;

    /// Sign data with the specified key
    /// Returns 64-byte signature (r || s) for secp256r1
    async fn sign(&self, key_id: &str, data: &[u8]) -> HsmResult<Vec<u8>>;

    /// Get the public key for a key ID
    async fn get_public_key(&self, key_id: &str) -> HsmResult<Vec<u8>>;

    /// Verify the HSM device is genuine (attestation)
    async fn verify_device(&self) -> HsmResult<bool>;

    /// Get the default key ID (first available key)
    async fn default_key_id(&self) -> HsmResult<String> {
        let keys = self.list_keys().await?;
        keys.first()
            .map(|k| k.key_id.clone())
            .ok_or_else(|| crate::error::HsmError::KeyNotFound("No keys available".to_string()))
    }
}

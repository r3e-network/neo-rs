//! HSM Signer trait definition

use crate::device::HsmDeviceInfo;
use crate::error::{HsmError, HsmResult};
use async_trait::async_trait;
use neo_crypto::Crypto;
use serde::{Deserialize, Serialize};

/// Information about a key stored in the HSM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmKeyInfo {
    /// Key identifier (device-specific)
    pub key_id: String,

    /// Public key (compressed, 33 bytes for secp256r1)
    pub public_key: Vec<u8>,

    /// Neo script hash derived from the signature redeem script (20 bytes)
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
    pub fn new(key_id: impl Into<String>, public_key: Vec<u8>, script_hash: [u8; 20]) -> Self {
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
    pub fn neo_address(&self, address_version: u8) -> String {
        let mut data = vec![address_version];
        data.extend_from_slice(&self.script_hash);

        // Double SHA256 for checksum
        let hash1 = Crypto::sha256(&data);
        let hash2 = Crypto::sha256(&hash1);
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

    /// Sign Neo message bytes with the specified key.
    ///
    /// Implementations must apply Neo's ECDsa.SignData semantics (SHA-256 over `data`)
    /// and return a 64-byte `r || s` signature for secp256r1.
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

pub(crate) fn script_hash_from_public_key(public_key: &[u8]) -> HsmResult<[u8; 20]> {
    let script = signature_redeem_script(public_key)?;
    Ok(Crypto::hash160(&script))
}

pub(crate) fn signature_redeem_script(public_key: &[u8]) -> HsmResult<Vec<u8>> {
    let compressed = normalize_public_key(public_key)?;
    if compressed.len() != 33 || (compressed[0] != 0x02 && compressed[0] != 0x03) {
        return Err(HsmError::InvalidKeyFormat(
            "Public key must be 33-byte compressed secp256r1".to_string(),
        ));
    }

    let mut script = Vec::with_capacity(40);
    script.push(0x0C); // PUSHDATA1
    script.push(compressed.len() as u8);
    script.extend_from_slice(&compressed);
    script.push(0x41); // SYSCALL
    script.extend_from_slice(&check_sig_hash());
    Ok(script)
}

pub(crate) fn normalize_public_key(public_key: &[u8]) -> HsmResult<Vec<u8>> {
    match public_key.len() {
        33 if public_key[0] == 0x02 || public_key[0] == 0x03 => Ok(public_key.to_vec()),
        65 if public_key[0] == 0x04 => Ok(compress_uncompressed_key(&public_key[1..])?),
        64 => Ok(compress_uncompressed_key(public_key)?),
        _ => Err(HsmError::InvalidKeyFormat(
            "Unsupported public key format".to_string(),
        )),
    }
}

fn compress_uncompressed_key(uncompressed_xy: &[u8]) -> HsmResult<Vec<u8>> {
    if uncompressed_xy.len() != 64 {
        return Err(HsmError::InvalidKeyFormat(
            "Uncompressed public key must be 64 bytes of X||Y".to_string(),
        ));
    }

    let x = &uncompressed_xy[..32];
    let y = &uncompressed_xy[32..];
    let y_last = y[31];
    let prefix = if y_last % 2 == 0 { 0x02 } else { 0x03 };
    let mut compressed = Vec::with_capacity(33);
    compressed.push(prefix);
    compressed.extend_from_slice(x);
    Ok(compressed)
}

fn check_sig_hash() -> [u8; 4] {
    let digest = Crypto::sha256(b"System.Crypto.CheckSig");
    [digest[0], digest[1], digest[2], digest[3]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::Secp256r1Crypto;

    const SAMPLE_PRIVATE_KEY: [u8; 32] = [1u8; 32];
    const SAMPLE_SCRIPT_HASH: &str = "6380ce3d7de7855bc5c1076d3b515eda380d2e90";

    #[test]
    fn script_hash_matches_neo_signature_contract() {
        let public_key = Secp256r1Crypto::derive_public_key(&SAMPLE_PRIVATE_KEY).expect("pubkey");
        let script_hash = script_hash_from_public_key(&public_key).expect("script hash");
        assert_eq!(hex::encode(script_hash), SAMPLE_SCRIPT_HASH);
        assert_ne!(script_hash, Crypto::hash160(&public_key));
    }
}

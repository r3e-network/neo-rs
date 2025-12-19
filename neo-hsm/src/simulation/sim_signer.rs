//! Simulation HSM signer for testing without hardware

use crate::device::HsmDeviceInfo;
use crate::error::{HsmError, HsmResult};
use crate::signer::{normalize_public_key, script_hash_from_public_key, HsmKeyInfo, HsmSigner};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use zeroize::Zeroizing;

/// Simulated HSM for testing without hardware
///
/// This implementation stores keys in memory and provides
/// the same interface as hardware HSMs for testing purposes.
pub struct SimulationSigner {
    device_info: HsmDeviceInfo,
    is_ready: RwLock<bool>,
    is_locked: RwLock<bool>,
    keys: RwLock<HashMap<String, SimulatedKey>>,
    pin: RwLock<Option<String>>,
}

struct SimulatedKey {
    private_key: Zeroizing<[u8; 32]>,
    public_key: Vec<u8>,
    script_hash: [u8; 20],
    label: Option<String>,
}

impl SimulationSigner {
    /// Create a new simulation signer
    pub fn new() -> Self {
        Self {
            device_info: HsmDeviceInfo::simulation(),
            is_ready: RwLock::new(false),
            is_locked: RwLock::new(false), // Simulation doesn't require PIN by default
            keys: RwLock::new(HashMap::new()),
            pin: RwLock::new(None),
        }
    }

    /// Create a simulation signer with a pre-generated test key
    pub fn with_test_key() -> HsmResult<Self> {
        let signer = Self::new();
        signer.generate_key("test-key-0", Some("Test Key"))?;
        *signer.is_ready.write() = true;
        Ok(signer)
    }

    /// Generate a new key pair
    pub fn generate_key(&self, key_id: &str, label: Option<&str>) -> HsmResult<HsmKeyInfo> {
        use neo_crypto::Secp256r1Crypto;

        use rand::RngCore;

        // Generate random private key
        let mut private_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut private_key);

        // Derive public key
        let public_key = Secp256r1Crypto::derive_public_key(&private_key)
            .map_err(|e| HsmError::CryptoError(format!("Failed to derive public key: {}", e)))?;

        let public_key = normalize_public_key(&public_key)?;
        let script_hash = script_hash_from_public_key(&public_key)?;

        let key_info = HsmKeyInfo::new(key_id, public_key.clone(), script_hash);
        let key_info = if let Some(l) = label {
            key_info.with_label(l)
        } else {
            key_info
        };

        // Store the key
        let sim_key = SimulatedKey {
            private_key: Zeroizing::new(private_key),
            public_key,
            script_hash,
            label: label.map(String::from),
        };

        self.keys.write().insert(key_id.to_string(), sim_key);

        Ok(key_info)
    }

    /// Import an existing private key
    pub fn import_key(
        &self,
        key_id: &str,
        private_key: &[u8],
        label: Option<&str>,
    ) -> HsmResult<HsmKeyInfo> {
        use neo_crypto::Secp256r1Crypto;

        if private_key.len() != 32 {
            return Err(HsmError::InvalidKeyFormat(
                "Private key must be 32 bytes".to_string(),
            ));
        }

        let mut pk = [0u8; 32];
        pk.copy_from_slice(private_key);

        // Derive public key
        let public_key = Secp256r1Crypto::derive_public_key(&pk)
            .map_err(|e| HsmError::CryptoError(format!("Failed to derive public key: {}", e)))?;

        let public_key = normalize_public_key(&public_key)?;
        let script_hash = script_hash_from_public_key(&public_key)?;

        let key_info = HsmKeyInfo::new(key_id, public_key.clone(), script_hash);
        let key_info = if let Some(l) = label {
            key_info.with_label(l)
        } else {
            key_info
        };

        // Store the key
        let sim_key = SimulatedKey {
            private_key: Zeroizing::new(pk),
            public_key,
            script_hash,
            label: label.map(String::from),
        };

        self.keys.write().insert(key_id.to_string(), sim_key);

        Ok(key_info)
    }

    /// Set a PIN for the simulation (for testing PIN flows)
    pub fn set_pin(&self, pin: &str) {
        *self.pin.write() = Some(pin.to_string());
        *self.is_locked.write() = true;
        let _ = &self.device_info; // Silence unused field warning
    }
}

impl Default for SimulationSigner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HsmSigner for SimulationSigner {
    fn device_info(&self) -> &HsmDeviceInfo {
        &self.device_info
    }

    fn is_ready(&self) -> bool {
        *self.is_ready.read()
    }

    async fn unlock(&self, pin: &str) -> HsmResult<()> {
        let stored_pin = self.pin.read();
        if let Some(ref expected) = *stored_pin {
            if pin != expected {
                return Err(HsmError::InvalidPin);
            }
        }
        // No PIN set or PIN matches
        *self.is_locked.write() = false;
        *self.is_ready.write() = true;
        Ok(())
    }

    fn lock(&self) {
        *self.is_locked.write() = true;
    }

    fn is_locked(&self) -> bool {
        *self.is_locked.read()
    }

    async fn list_keys(&self) -> HsmResult<Vec<HsmKeyInfo>> {
        let keys = self.keys.read();
        Ok(keys
            .iter()
            .map(|(id, key)| {
                let mut info = HsmKeyInfo::new(id.clone(), key.public_key.clone(), key.script_hash);
                if let Some(ref label) = key.label {
                    info = info.with_label(label.clone());
                }
                info
            })
            .collect())
    }

    async fn get_key(&self, key_id: &str) -> HsmResult<HsmKeyInfo> {
        let keys = self.keys.read();
        let key = keys
            .get(key_id)
            .ok_or_else(|| HsmError::KeyNotFound(key_id.to_string()))?;

        let mut info = HsmKeyInfo::new(key_id, key.public_key.clone(), key.script_hash);
        if let Some(ref label) = key.label {
            info = info.with_label(label.clone());
        }
        Ok(info)
    }

    async fn sign(&self, key_id: &str, data: &[u8]) -> HsmResult<Vec<u8>> {
        use neo_crypto::Secp256r1Crypto;

        if self.is_locked() {
            return Err(HsmError::PinRequired);
        }

        let keys = self.keys.read();
        let key = keys
            .get(key_id)
            .ok_or_else(|| HsmError::KeyNotFound(key_id.to_string()))?;

        let signature = Secp256r1Crypto::sign(data, &key.private_key)
            .map_err(|e| HsmError::SigningFailed(e.to_string()))?;

        Ok(signature.to_vec())
    }

    async fn get_public_key(&self, key_id: &str) -> HsmResult<Vec<u8>> {
        let keys = self.keys.read();
        let key = keys
            .get(key_id)
            .ok_or_else(|| HsmError::KeyNotFound(key_id.to_string()))?;

        Ok(key.public_key.clone())
    }

    async fn verify_device(&self) -> HsmResult<bool> {
        // Simulation always passes verification
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_signer_basic() {
        let signer = SimulationSigner::new();

        // Generate a key
        let key_info = signer
            .generate_key("test-key", Some("Test Key"))
            .expect("Failed to generate key");

        assert_eq!(key_info.key_id, "test-key");
        assert_eq!(key_info.public_key.len(), 33); // Compressed public key
        assert_eq!(key_info.label, Some("Test Key".to_string()));

        // Unlock (no PIN set)
        signer.unlock("").await.expect("Failed to unlock");
        assert!(signer.is_ready());

        // Sign data
        let data = b"test data to sign";
        let signature = signer.sign("test-key", data).await.expect("Failed to sign");

        assert_eq!(signature.len(), 64); // r || s
    }

    #[tokio::test]
    async fn test_simulation_signer_with_pin() {
        let signer = SimulationSigner::new();
        signer
            .generate_key("key1", None)
            .expect("Failed to generate key");
        signer.set_pin("1234");

        // Should be locked
        assert!(signer.is_locked());

        // Wrong PIN should fail
        let result = signer.unlock("wrong").await;
        assert!(matches!(result, Err(HsmError::InvalidPin)));

        // Correct PIN should work
        signer.unlock("1234").await.expect("Failed to unlock");
        assert!(!signer.is_locked());
    }

    #[tokio::test]
    async fn test_list_keys() {
        let signer = SimulationSigner::new();
        signer.generate_key("key1", Some("Key 1")).unwrap();
        signer.generate_key("key2", Some("Key 2")).unwrap();

        signer.unlock("").await.unwrap();

        let keys = signer.list_keys().await.unwrap();
        assert_eq!(keys.len(), 2);
    }
}

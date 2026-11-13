//! Sealed key storage for TEE wallet

use crate::enclave::{SealedData, TeeEnclave, rollback_floor, seal_data, unseal_data};
use crate::error::{TeeError, TeeResult};
use neo_crypto::Base58;
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

        let sealed_data = seal_data(private_key, &sealing_key, &aad, counter)?;

        // T43: raise the EXTERNAL rollback floor before handing out the blob.
        // The floor is persisted next to the sealed blobs (not inside this
        // object), so a later rollback of the host-side counter state can be
        // detected at unseal time. Fail closed: if the floor cannot be loaded
        // (corrupt) or persisted, the seal is refused.
        let floor_path = rollback_floor::floor_path(enclave.config().sealed_data_path.as_path());
        let current_floor = rollback_floor::load(&floor_path)?.unwrap_or(0);
        if counter > current_floor {
            rollback_floor::store(&floor_path, counter)?;
        }

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
    ///
    /// # Rollback protection (T43)
    ///
    /// The replay floor is an EXTERNAL witness (`.rollback_floor` in the
    /// enclave's sealed-data directory), no longer the blob's own counter
    /// — the previous `min_counter = Some(sealed_data.counter)` was
    /// self-referential and could never reject (T07-T4). Acceptance requires:
    ///
    /// ```text
    /// blob.counter <= floor <= enclave.current_counter()
    /// ```
    ///
    /// * `floor > enclave counter` — the persisted counter state was rolled
    ///   back relative to the floor witness (e.g. the counter file was
    ///   deleted/restored): reject, fail closed.
    /// * `blob.counter > floor` — the blob is not covered by the floor
    ///   history (floor file rolled back, or a blob from another directory):
    ///   reject, fail closed.
    /// * corrupt floor file — reject, never treat as 0.
    /// * missing floor file — contradiction while a sealed blob exists:
    ///   reject. The floor is created on first boot by `TeeEnclave::initialize`
    ///   and raised by every `seal`, so a missing floor at unseal time means
    ///   tampering or a pre-T43 blob (migration is rejected conservatively).
    ///
    /// In simulation mode this is best-effort, file-level rollback detection;
    /// a coherent rollback of the whole sealed-data directory cannot be
    /// caught without SGX hardware monotonic counters (see
    /// [`crate::enclave::rollback_floor`] for the threat model). On the strict
    /// SGX hardware path the same file-based floor applies as
    /// defense-in-depth; hardware monotonic counters are not yet integrated.
    pub fn unseal(&self, enclave: &TeeEnclave) -> TeeResult<Zeroizing<Vec<u8>>> {
        let sealing_key = enclave.sealing_key()?;

        // T43: load the external floor witness and enforce it against both
        // the blob and the enclave's live counter state.
        let floor_path = rollback_floor::floor_path(enclave.config().sealed_data_path.as_path());
        let enclave_counter = enclave.current_counter()?;
        let floor = rollback_floor::load(&floor_path)?.ok_or_else(|| {
            TeeError::RollbackProtection(format!(
                "rollback floor witness {floor_path:?} is missing while a sealed blob exists \
                 (tampering or pre-T43 blob); refusing to unseal (fail-closed)"
            ))
        })?;

        if floor > enclave_counter {
            return Err(TeeError::RollbackProtection(format!(
                "sealed-data state was rolled back: rollback floor {floor} is ahead of the \
                 enclave monotonic counter {enclave_counter}; refusing to unseal (fail-closed)"
            )));
        }

        if self.sealed_data.counter > floor {
            return Err(TeeError::RollbackProtection(format!(
                "sealed blob counter {} is ahead of the rollback floor {}; the floor \
                 witness was rolled back or the blob is foreign; refusing to unseal (fail-closed)",
                self.sealed_data.counter, floor
            )));
        }

        // Verify AAD matches
        let mut expected_aad = Vec::new();
        expected_aad.extend_from_slice(&self.public_key);
        expected_aad.extend_from_slice(&self.script_hash);

        if self.sealed_data.aad != expected_aad {
            return Err(TeeError::UnsealingFailed(
                "AAD mismatch - possible tampering".to_string(),
            ));
        }

        // The counter is covered by the AEAD tag (sealing.rs binds it into
        // the AAD), so a tampered `SealedData::counter` field fails decryption
        // here instead of weakening replay protection.
        let plaintext = unseal_data(
            &self.sealed_data,
            &sealing_key,
            Some(self.sealed_data.counter),
        )?;

        // The acceptance checks above guarantee `blob.counter <= floor`, and
        // `seal` raises the floor before handing out a blob, so the floor
        // dominates every counter covered by a sealed key. No post-unseal
        // write is needed (and writing here would be dead code).

        Ok(Zeroizing::new(plaintext))
    }

    /// Save sealed key to file
    pub fn save_to_file(&self, path: &Path) -> TeeResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        crate::fs_acl::write_owner_only(path, json.as_bytes())?;
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
        Base58::encode_check(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use tempfile::tempdir;

    fn sealed_key_with_script_hash(script_hash: [u8; 20]) -> SealedKey {
        SealedKey {
            sealed_data: SealedData {
                ciphertext: Vec::new(),
                nonce: [0; 12],
                aad: Vec::new(),
                counter: 0,
                version: SealedData::CURRENT_VERSION,
                context: None,
            },
            public_key: Vec::new(),
            label: None,
            script_hash,
            created_at: 0,
        }
    }

    #[test]
    fn address_uses_base58_check_with_neo_n3_version() {
        let script_hash = [0xAB; 20];
        let sealed = sealed_key_with_script_hash(script_hash);
        let address = sealed.address();

        let mut payload = vec![0x35];
        payload.extend_from_slice(&script_hash);

        assert_eq!(address, Base58::encode_check(&payload));
        assert_eq!(Base58::decode_check(&address).unwrap(), payload);
    }

    #[test]
    fn address_preserves_zero_script_hash_payload() {
        let sealed = sealed_key_with_script_hash([0; 20]);
        let address = sealed.address();

        let mut payload = vec![0x35];
        payload.extend_from_slice(&[0; 20]);

        assert_eq!(Base58::decode_check(&address).unwrap(), payload);
    }

    #[test]
    fn test_seal_unseal_key() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        enclave.initialize().unwrap();

        let private_key = [0x42u8; 32];
        let public_key = [0x02u8; 33]; // Compressed public key format
        let script_hash = [0xABu8; 20];

        let sealed = SealedKey::seal(
            &enclave,
            &private_key,
            &public_key,
            &script_hash,
            Some("test-key".to_string()),
        )
        .unwrap();

        let unsealed = sealed.unseal(&enclave).unwrap();
        assert_eq!(&*unsealed, &private_key);
    }

    #[test]
    fn test_save_load_sealed_key() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        enclave.initialize().unwrap();

        let private_key = [0x42u8; 32];
        let public_key = [0x02u8; 33];
        let script_hash = [0xABu8; 20];

        let sealed =
            SealedKey::seal(&enclave, &private_key, &public_key, &script_hash, None).unwrap();

        let key_path = temp.path().join("test_key.json");
        sealed.save_to_file(&key_path).unwrap();

        let loaded = SealedKey::load_from_file(&key_path).unwrap();
        let unsealed = loaded.unseal(&enclave).unwrap();
        assert_eq!(&*unsealed, &private_key);
    }
}

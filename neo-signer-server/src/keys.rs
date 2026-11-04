//! Sealed signing-key management for the reference signer.
//!
//! The P-256 signing key is generated once at first boot, sealed with
//! [`neo_tee::SealedKey`] (AEAD under the enclave sealing key with the
//! rollback-floor witness raised before the blob is handed out) and persisted
//! to disk. On every later boot the blob is loaded and unsealed; the
//! rollback-floor acceptance rule
//! `blob.counter <= floor <= enclave.current_counter()` from the neo-tee
//! enclave is enforced fail-closed inside [`SealedKey::unseal`], so a rolled
//! back sealed-data directory cannot silently revive an old key.

use anyhow::{Context, Result, bail};
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_tee::{EnclaveConfig, SealedKey, TeeEnclave};
use std::path::Path;
use std::sync::Arc;
use zeroize::Zeroizing;

/// A server signing key whose private material only ever exists unsealed in
/// memory for the duration of one signing operation.
pub struct SealedSigningKey {
    enclave: Arc<TeeEnclave>,
    sealed: SealedKey,
}

impl SealedSigningKey {
    /// Loads the sealed key from `key_path`, or generates and seals a fresh
    /// P-256 key on first boot.
    ///
    /// The enclave is a neo-tee **simulation** enclave in this reference
    /// implementation; a production deployment runs the same flow inside a
    /// hardware TEE.
    pub fn load_or_generate(sealed_data_dir: &Path, key_path: &Path, label: &str) -> Result<Self> {
        let enclave = Arc::new(TeeEnclave::new(EnclaveConfig {
            sealed_data_path: sealed_data_dir.to_path_buf(),
            simulation: true,
            ..EnclaveConfig::default()
        }));
        enclave
            .initialize()
            .context("failed to initialize the signer TEE enclave")?;

        let sealed = if key_path.exists() {
            SealedKey::load_from_file(key_path)
                .with_context(|| format!("failed to load sealed key {}", key_path.display()))?
        } else {
            let private_key = Secp256r1Crypto::generate_private_key();
            let public_key = Secp256r1Crypto::derive_public_key(&private_key)
                .context("failed to derive the signing public key")?;
            let script_hash = script_hash_for(&public_key);
            let sealed = SealedKey::seal(
                &enclave,
                &private_key,
                &public_key,
                &script_hash,
                Some(label.to_string()),
            )
            .context("failed to seal the freshly generated signing key")?;
            sealed
                .save_to_file(key_path)
                .with_context(|| format!("failed to persist sealed key {}", key_path.display()))?;
            sealed
        };

        // Fail closed: the persisted blob must unseal and reproduce exactly the
        // recorded public key before the server is allowed to start serving.
        let private_key = sealed
            .unseal(&enclave)
            .context("failed to unseal the signing key (rollback floor enforced)")?;
        let public_key = derive_public(&private_key)?;
        if public_key != sealed.public_key {
            bail!("unsealed signing key does not match the recorded public key");
        }

        Ok(Self { enclave, sealed })
    }

    /// Compressed P-256 public key bytes (33 bytes) served through key-info.
    #[must_use]
    pub fn public_key(&self) -> &[u8] {
        &self.sealed.public_key
    }

    /// The neo-tee enclave guarding the sealed key.
    #[must_use]
    pub fn enclave(&self) -> &Arc<TeeEnclave> {
        &self.enclave
    }

    /// Signs a 32-byte digest, unsealing the private key only for the
    /// duration of this call.
    pub fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; 64]> {
        let private_key: Zeroizing<Vec<u8>> = self
            .sealed
            .unseal(&self.enclave)
            .context("failed to unseal the signing key for signing")?;
        let private_key: [u8; 32] = private_key
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("unsealed private key must be exactly 32 bytes"))?;
        Secp256r1Crypto::sign_prehash(digest, &private_key)
            .context("failed to produce the P-256 signature")
    }
}

fn derive_public(private_key: &[u8]) -> Result<Vec<u8>> {
    let private_key: [u8; 32] = private_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("unsealed private key must be exactly 32 bytes"))?;
    Secp256r1Crypto::derive_public_key(&private_key)
        .context("failed to re-derive the signing public key")
}

/// Deterministic 20-byte binding for the SealedKey AAD. This is a reference
/// implementation; production deployments should use the Neo verification
/// script hash of the attested public key.
fn script_hash_for(public_key: &[u8]) -> [u8; 20] {
    let digest = Crypto::sha256(public_key);
    digest[..20]
        .try_into()
        .expect("the first 20 bytes of a 32-byte digest are exactly 20 bytes")
}

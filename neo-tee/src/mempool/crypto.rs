//! Content-encryption engine for the encrypted mempool (Privacy Mode Phase A).
//!
//! This module implements the "encrypted intake -> TEE decrypt -> ordered
//! hash table" link of Privacy Mode Phase A:
//!
//! 1. Transaction payloads are sealed at mempool intake under a fresh random
//!    AES-256-GCM *data key* ([`ContentCipher::seal`]).
//! 2. The data key itself is wrapped (KEM-style) by a [`KeyEncapsulation`]
//!    provider, in one of two opt-in modes:
//!    * [`KeyEncapsulation::CommitteeKey`] — a simulated Keypers committee
//!      epoch key encapsulates the data key. Phase A implements *single
//!      epoch key encapsulation*; splitting the epoch secret into `t`-of-`n`
//!      threshold shares (Shamir secret sharing or a threshold-encryption
//!      scheme such as Shoup-style threshold BLS) is deliberately left as a
//!      future extension — the [`KeyEncapsulation`] and [`MempoolCiphertext`]
//!      interfaces are the seams where share splitting plugs in.
//!    * [`KeyEncapsulation::TeeSealed`] — the enclave sealing key (via
//!      [`crate::enclave::seal_data`]) encapsulates the data key, with the
//!      same external rollback-floor witness checks as
//!      [`crate::wallet::SealedKey`], so a rolled-back floor/counter state
//!      fails closed at decapsulation time.
//! 3. Decryption goes through the mempool content state machine
//!    (`Encrypted -> Resolved | Rejected`); AEAD tag verification failure or
//!    decapsulation failure rejects the entry fail-closed (see
//!    `crate::mempool::tee_mempool`).
//!
//! # Ciphertext envelope
//!
//! [`MempoolCiphertext`] is encoded as a compact binary envelope in the
//! crate's existing `to_bytes`/`from_bytes` style:
//!
//! ```text
//! offset  field
//! 0       version (u8)                = CIPHERTEXT_VERSION
//! 1       KEM mode (u8)               = MODE_COMMITTEE_KEY | MODE_TEE_SEALED_KEY
//! 2..10   epoch id (u64, little-endian)
//! 10..12  encapsulated key material length (u16, little-endian)
//! 12..    encapsulated key material (KEM-specific)
//! ..+12   AEAD nonce (12 bytes)
//! ..+4    ciphertext length (u32, little-endian)
//! ..      AES-256-GCM ciphertext || 16-byte tag
//! ```
//!
//! The envelope header (version, mode, epoch id) and the transaction hash are
//! bound into the AEAD additional authenticated data on both layers, so
//! header-field tampering, cross-epoch replay, and hash substitution all fail
//! authentication.
//!
//! # Simulation caveat (MUST READ)
//!
//! Under the `simulation` feature (and on the current strict-hardware path,
//! which still runs in the host process) this engine provides *interface and
//! flow verification only*: the keys and the plaintext live in ordinary host
//! memory, so host-side encryption does NOT provide real enclave isolation
//! guarantees. Production deployments require a real TEE path where payload
//! decryption, the data keys, and the resolved plaintext exist only inside
//! the enclave boundary. Phase A deliberately covers only the
//! "encrypted intake -> TEE decrypt -> ordered hash table" link.

use crate::enclave::{
    KeyDerivationParams, SealedData, TeeEnclave, derive_key_hkdf, rollback_floor, seal_data,
    unseal_data,
};
use crate::error::{TeeError, TeeResult};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::RngCore;
use rand::rngs::OsRng;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Version byte of the [`MempoolCiphertext`] envelope format.
pub const CIPHERTEXT_VERSION: u8 = 1;

/// KEM mode byte: simulated committee epoch-key encapsulation.
pub const MODE_COMMITTEE_KEY: u8 = 0;

/// KEM mode byte: enclave-sealed key encapsulation (rollback-floor protected).
pub const MODE_TEE_SEALED_KEY: u8 = 1;

/// Epoch id used by the TEE-sealed KEM mode (the mode has no epoch rotation).
pub const TEE_SEALED_EPOCH_ID: u64 = 0;

/// AES-256 data-key length.
const DATA_KEY_LEN: usize = 32;

/// AES-GCM nonce length.
const NONCE_LEN: usize = 12;

/// Fixed envelope header length: version(1) + mode(1) + epoch id(8) + kem len(2).
const ENVELOPE_HEADER_LEN: usize = 12;

/// HKDF context used to derive the committee epoch wrapping key.
const COMMITTEE_KEM_CONTEXT: &str = "neo-tee-mempool-committee-epoch-v1";

/// A simulated Keypers committee epoch key.
///
/// Phase A implements *single epoch key* encapsulation: the epoch "public
/// key" is the symmetric epoch key itself, because in simulation there is no
/// committee to distribute shares to. The decapsulation holder must possess
/// the epoch secret.
///
/// # Future work: t-of-n threshold shares
///
/// The production Keypers-style design splits the epoch secret into `n`
/// shares (Shamir secret sharing, or a native threshold-encryption scheme
/// such as threshold BLS/Shoup) so that any `t` committee members can combine
/// their decapsulation shares to recover the data key while no fewer than
/// `t` can. The intended interface is:
///
/// ```text
/// CommitteeEpochKey::generate(epoch_id)
///     .split_into_shares(t, n) -> (epoch_public_key, Vec<Share>)
/// CommitteeShares::combine(shares) -> DecapsulationKey
/// ```
///
/// Only the `generate`/encapsulate/decapsulate single-key surface is
/// implemented here; share splitting is deliberately deferred.
pub struct CommitteeEpochKey {
    epoch_id: u64,
    secret: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for CommitteeEpochKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the secret, even in trace logs.
        f.debug_struct("CommitteeEpochKey")
            .field("epoch_id", &self.epoch_id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl CommitteeEpochKey {
    /// Generate a fresh random epoch key for the given epoch.
    pub fn generate(epoch_id: u64) -> Self {
        let mut secret = [0u8; DATA_KEY_LEN];
        OsRng.fill_bytes(&mut secret);
        Self {
            epoch_id,
            secret: Zeroizing::new(secret),
        }
    }

    /// Build an epoch key from explicit secret material (provisioning/tests).
    pub fn from_secret(epoch_id: u64, secret: [u8; 32]) -> Self {
        Self {
            epoch_id,
            secret: Zeroizing::new(secret),
        }
    }

    /// The epoch this key is valid for.
    pub fn epoch_id(&self) -> u64 {
        self.epoch_id
    }

    /// The epoch public key material.
    ///
    /// Simulation caveat: Phase A uses a symmetric epoch key, so the "public
    /// key" IS the secret. A production KEM replaces this with an asymmetric
    /// (or threshold) public key that can encapsulate without being able to
    /// decapsulate; the caller-visible shape stays identical.
    pub fn public_key(&self) -> [u8; 32] {
        *self.secret
    }

    /// Derive the AES-256 wrapping key for this epoch via HKDF-SHA256.
    fn wrapping_key(&self) -> TeeResult<[u8; 32]> {
        let params = KeyDerivationParams {
            base_key: &self.secret,
            context: COMMITTEE_KEM_CONTEXT,
            salt: Some(b"neo-tee-mempool-kem"),
        };
        derive_key_hkdf(params)
    }

    /// Wrap (encapsulate) a data key under the epoch key.
    fn encapsulate(&self, data_key: &[u8; DATA_KEY_LEN], tx_hash: &[u8; 32]) -> TeeResult<Vec<u8>> {
        let wrapping_key = self.wrapping_key()?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&wrapping_key).map_err(|e| {
            TeeError::CryptoError(format!("failed to build committee KEM cipher: {e}"))
        })?;
        let aad = committee_kem_aad(self.epoch_id, tx_hash);
        let wrapped = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: data_key,
                    aad: &aad,
                },
            )
            .map_err(|e| {
                TeeError::SealingFailed(format!("committee KEM encapsulation failed: {e}"))
            })?;

        let mut material = Vec::with_capacity(NONCE_LEN + wrapped.len());
        material.extend_from_slice(&nonce_bytes);
        material.extend_from_slice(&wrapped);
        Ok(material)
    }

    /// Unwrap (decapsulate) a data key previously encapsulated for this epoch.
    fn decapsulate(
        &self,
        epoch_id: u64,
        material: &[u8],
        tx_hash: &[u8; 32],
    ) -> TeeResult<Zeroizing<[u8; DATA_KEY_LEN]>> {
        if epoch_id != self.epoch_id {
            return Err(TeeError::CryptoError(format!(
                "ciphertext epoch {epoch_id} does not match committee epoch {}",
                self.epoch_id
            )));
        }
        if material.len() < NONCE_LEN + DATA_KEY_LEN {
            return Err(TeeError::CryptoError(
                "committee KEM material is too short".to_string(),
            ));
        }

        let wrapping_key = self.wrapping_key()?;
        let cipher = Aes256Gcm::new_from_slice(&wrapping_key).map_err(|e| {
            TeeError::CryptoError(format!("failed to build committee KEM cipher: {e}"))
        })?;
        let aad = committee_kem_aad(self.epoch_id, tx_hash);
        let data_key = cipher
            .decrypt(
                Nonce::from_slice(&material[..NONCE_LEN]),
                Payload {
                    msg: &material[NONCE_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|e| {
                TeeError::UnsealingFailed(format!(
                    "committee KEM decapsulation failed (wrong epoch key or tampered material): {e}"
                ))
            })?;

        if data_key.len() != DATA_KEY_LEN {
            return Err(TeeError::UnsealingFailed(
                "committee KEM unwrapped a data key of unexpected length".to_string(),
            ));
        }

        let mut key = [0u8; DATA_KEY_LEN];
        key.copy_from_slice(&data_key);
        Ok(Zeroizing::new(key))
    }
}

/// KEM-style key encapsulation provider for mempool content encryption.
///
/// Wraps a per-payload random AES-256 data key so that only the configured
/// decapsulation authority (the simulated committee epoch, or the enclave
/// sealing key) can recover it.
pub enum KeyEncapsulation {
    /// Simulated Keypers committee: a single epoch key encapsulates data keys.
    CommitteeKey(CommitteeEpochKey),
    /// Enclave sealing key encapsulates data keys, guarded by the external
    /// rollback floor witness (same acceptance rule as `SealedKey::unseal`).
    TeeSealed(Arc<TeeEnclave>),
}

impl KeyEncapsulation {
    /// Envelope mode byte for this KEM mode.
    pub fn mode_byte(&self) -> u8 {
        match self {
            Self::CommitteeKey(_) => MODE_COMMITTEE_KEY,
            Self::TeeSealed(_) => MODE_TEE_SEALED_KEY,
        }
    }

    /// Envelope epoch id for this KEM mode.
    pub fn epoch_id(&self) -> u64 {
        match self {
            Self::CommitteeKey(key) => key.epoch_id(),
            Self::TeeSealed(_) => TEE_SEALED_EPOCH_ID,
        }
    }

    /// Encapsulate a data key, returning mode-specific KEM material.
    fn encapsulate(&self, data_key: &[u8; DATA_KEY_LEN], tx_hash: &[u8; 32]) -> TeeResult<Vec<u8>> {
        match self {
            Self::CommitteeKey(key) => key.encapsulate(data_key, tx_hash),
            Self::TeeSealed(enclave) => {
                // Each encapsulation burns one monotonic counter tick and the
                // rollback floor is raised to cover it (mirrors
                // `SealedKey::seal`), so a later rollback of the floor/counter
                // state fails closed at decapsulation time.
                let counter = enclave.increment_counter()?;
                let sealing_key = enclave.sealing_key()?;
                let aad = sealed_kem_aad(tx_hash);
                let sealed = seal_data(data_key, &sealing_key, &aad, counter)?;

                let floor_path =
                    rollback_floor::floor_path(enclave.config().sealed_data_path.as_path());
                let current_floor = rollback_floor::load(&floor_path)?.unwrap_or(0);
                if counter > current_floor {
                    rollback_floor::store(&floor_path, counter)?;
                }

                sealed.to_bytes()
            }
        }
    }

    /// Decapsulate the data key from KEM material previously produced by an
    /// encapsulation with the same mode, epoch, and transaction hash.
    fn decapsulate(
        &self,
        mode: u8,
        epoch_id: u64,
        material: &[u8],
        tx_hash: &[u8; 32],
    ) -> TeeResult<Zeroizing<[u8; DATA_KEY_LEN]>> {
        match self {
            Self::CommitteeKey(key) => {
                if mode != MODE_COMMITTEE_KEY {
                    return Err(TeeError::CryptoError(format!(
                        "KEM mode mismatch: committee provider cannot decapsulate mode {mode}"
                    )));
                }
                key.decapsulate(epoch_id, material, tx_hash)
            }
            Self::TeeSealed(enclave) => {
                if mode != MODE_TEE_SEALED_KEY {
                    return Err(TeeError::CryptoError(format!(
                        "KEM mode mismatch: tee-sealed provider cannot decapsulate mode {mode}"
                    )));
                }
                if epoch_id != TEE_SEALED_EPOCH_ID {
                    return Err(TeeError::CryptoError(format!(
                        "tee-sealed KEM has no epoch rotation but ciphertext claims epoch {epoch_id}"
                    )));
                }

                let sealed = SealedData::from_bytes(material)?;

                // Rollback-floor acceptance, identical to `SealedKey::unseal`:
                // the blob counter must be covered by the external floor
                // witness, and the floor must not be ahead of the live
                // enclave counter. Any inconsistency fails closed.
                let floor_path =
                    rollback_floor::floor_path(enclave.config().sealed_data_path.as_path());
                let enclave_counter = enclave.current_counter()?;
                let floor = rollback_floor::load(&floor_path)?.ok_or_else(|| {
                    TeeError::RollbackProtection(format!(
                        "rollback floor witness {:?} is missing while a sealed KEM blob exists; \
                         refusing to decapsulate (fail-closed)",
                        floor_path
                    ))
                })?;

                if floor > enclave_counter {
                    return Err(TeeError::RollbackProtection(format!(
                        "sealed-data state was rolled back: rollback floor {} is ahead of the \
                         enclave monotonic counter {}; refusing to decapsulate (fail-closed)",
                        floor, enclave_counter
                    )));
                }

                if sealed.counter > floor {
                    return Err(TeeError::RollbackProtection(format!(
                        "sealed KEM blob counter {} is ahead of the rollback floor {}; the floor \
                         witness was rolled back or the blob is foreign; refusing to decapsulate \
                         (fail-closed)",
                        sealed.counter, floor
                    )));
                }

                let sealing_key = enclave.sealing_key()?;
                let data_key = unseal_data(&sealed, &sealing_key, Some(sealed.counter))?;
                if data_key.len() != DATA_KEY_LEN {
                    return Err(TeeError::UnsealingFailed(
                        "sealed KEM unwrapped a data key of unexpected length".to_string(),
                    ));
                }

                let mut key = [0u8; DATA_KEY_LEN];
                key.copy_from_slice(&data_key);
                Ok(Zeroizing::new(key))
            }
        }
    }
}

/// Encrypted mempool payload envelope.
///
/// Binary layout (little-endian) — see the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MempoolCiphertext {
    /// Envelope format version (`CIPHERTEXT_VERSION`).
    pub version: u8,
    /// KEM mode byte (`MODE_COMMITTEE_KEY` or `MODE_TEE_SEALED_KEY`).
    pub mode: u8,
    /// Epoch id the data key was encapsulated for.
    pub epoch_id: u64,
    /// Mode-specific encapsulated key material.
    pub encapsulated_key: Vec<u8>,
    /// AES-256-GCM nonce for the payload layer.
    pub nonce: [u8; NONCE_LEN],
    /// AES-256-GCM ciphertext || 16-byte tag over the payload.
    pub ciphertext: Vec<u8>,
}

impl MempoolCiphertext {
    /// Encode the envelope into the compact binary wire format.
    pub fn encode(&self) -> TeeResult<Vec<u8>> {
        if self.encapsulated_key.len() > u16::MAX as usize {
            return Err(TeeError::SerializationError(
                "encapsulated key material exceeds u16 length limit".to_string(),
            ));
        }
        if self.ciphertext.len() > u32::MAX as usize {
            return Err(TeeError::SerializationError(
                "ciphertext exceeds u32 length limit".to_string(),
            ));
        }

        let mut out = Vec::with_capacity(
            ENVELOPE_HEADER_LEN
                + self.encapsulated_key.len()
                + NONCE_LEN
                + 4
                + self.ciphertext.len(),
        );
        out.push(self.version);
        out.push(self.mode);
        out.extend_from_slice(&self.epoch_id.to_le_bytes());
        out.extend_from_slice(&(self.encapsulated_key.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.encapsulated_key);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&(self.ciphertext.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.ciphertext);
        Ok(out)
    }

    /// Decode and strictly validate an envelope. Any length, version, or mode
    /// deviation fails closed with a serialization error.
    pub fn decode(bytes: &[u8]) -> TeeResult<Self> {
        if bytes.len() < ENVELOPE_HEADER_LEN {
            return Err(TeeError::SerializationError(
                "mempool ciphertext envelope is too short".to_string(),
            ));
        }

        let version = bytes[0];
        if version != CIPHERTEXT_VERSION {
            return Err(TeeError::SerializationError(format!(
                "unsupported mempool ciphertext version {version}"
            )));
        }

        let mode = bytes[1];
        if mode != MODE_COMMITTEE_KEY && mode != MODE_TEE_SEALED_KEY {
            return Err(TeeError::SerializationError(format!(
                "unknown mempool ciphertext KEM mode {mode}"
            )));
        }

        let epoch_id = u64::from_le_bytes(
            bytes[2..10]
                .try_into()
                .expect("slice between offsets 2 and 10 has exactly 8 bytes"),
        );
        let kem_len = u16::from_le_bytes(
            bytes[10..12]
                .try_into()
                .expect("slice between offsets 10 and 12 has exactly 2 bytes"),
        );

        let mut offset = ENVELOPE_HEADER_LEN;
        if bytes.len() < offset + kem_len as usize {
            return Err(TeeError::SerializationError(
                "mempool ciphertext envelope ends inside the KEM material".to_string(),
            ));
        }
        let encapsulated_key = bytes[offset..offset + kem_len as usize].to_vec();
        offset += kem_len as usize;

        if bytes.len() < offset + NONCE_LEN {
            return Err(TeeError::SerializationError(
                "mempool ciphertext envelope ends inside the nonce".to_string(),
            ));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[offset..offset + NONCE_LEN]);
        offset += NONCE_LEN;

        if bytes.len() < offset + 4 {
            return Err(TeeError::SerializationError(
                "mempool ciphertext envelope ends inside the ciphertext length".to_string(),
            ));
        }
        let ct_len = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("slice of exactly 4 bytes was just bounds-checked"),
        ) as usize;
        offset += 4;

        if bytes.len() != offset + ct_len {
            return Err(TeeError::SerializationError(
                "mempool ciphertext envelope length does not match its ciphertext length field"
                    .to_string(),
            ));
        }
        let ciphertext = bytes[offset..].to_vec();

        Ok(Self {
            version,
            mode,
            epoch_id,
            encapsulated_key,
            nonce,
            ciphertext,
        })
    }

    /// The authenticated header bytes: version || mode || epoch id.
    fn header_bytes(&self) -> [u8; 10] {
        let mut header = [0u8; 10];
        header[0] = self.version;
        header[1] = self.mode;
        header[2..].copy_from_slice(&self.epoch_id.to_le_bytes());
        header
    }
}

/// AEAD layer for mempool payload encryption.
///
/// Each payload is encrypted under a fresh random AES-256-GCM data key; the
/// data key is encapsulated by the configured [`KeyEncapsulation`] provider.
/// The envelope header and the transaction hash are authenticated on both the
/// payload layer and the KEM layer, binding each ciphertext to exactly one
/// transaction hash and epoch.
pub struct ContentCipher;

impl ContentCipher {
    /// Encrypt a transaction payload for `tx_hash` under the given KEM mode.
    pub fn seal(
        payload: &[u8],
        tx_hash: &[u8; 32],
        kem: &KeyEncapsulation,
    ) -> TeeResult<MempoolCiphertext> {
        let mut data_key = Zeroizing::new([0u8; DATA_KEY_LEN]);
        OsRng.fill_bytes(&mut data_key[..]);

        let encapsulated_key = kem.encapsulate(&data_key, tx_hash)?;

        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);

        let envelope = MempoolCiphertext {
            version: CIPHERTEXT_VERSION,
            mode: kem.mode_byte(),
            epoch_id: kem.epoch_id(),
            encapsulated_key,
            nonce,
            ciphertext: Vec::new(),
        };
        let aad = content_aad(&envelope.header_bytes(), tx_hash);

        let cipher = Aes256Gcm::new_from_slice(&data_key[..])
            .map_err(|e| TeeError::CryptoError(format!("failed to build content cipher: {e}")))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: payload,
                    aad: &aad,
                },
            )
            .map_err(|e| TeeError::SealingFailed(format!("content encryption failed: {e}")))?;

        Ok(MempoolCiphertext {
            ciphertext,
            ..envelope
        })
    }

    /// Decrypt a payload previously sealed with [`ContentCipher::seal`].
    ///
    /// Fails closed on any envelope, KEM, or AEAD-tag problem; never panics
    /// and never returns partially decrypted data.
    pub fn open(
        ciphertext: &MempoolCiphertext,
        tx_hash: &[u8; 32],
        kem: &KeyEncapsulation,
    ) -> TeeResult<Zeroizing<Vec<u8>>> {
        if ciphertext.version != CIPHERTEXT_VERSION {
            return Err(TeeError::SerializationError(format!(
                "unsupported mempool ciphertext version {}",
                ciphertext.version
            )));
        }
        if ciphertext.mode != kem.mode_byte() {
            return Err(TeeError::CryptoError(format!(
                "KEM mode mismatch: ciphertext mode {} cannot be opened by configured mode {}",
                ciphertext.mode,
                kem.mode_byte()
            )));
        }
        if ciphertext.epoch_id != kem.epoch_id() {
            return Err(TeeError::CryptoError(format!(
                "epoch mismatch: ciphertext epoch {} cannot be opened by configured epoch {}",
                ciphertext.epoch_id,
                kem.epoch_id()
            )));
        }

        let data_key = kem.decapsulate(
            ciphertext.mode,
            ciphertext.epoch_id,
            &ciphertext.encapsulated_key,
            tx_hash,
        )?;

        let aad = content_aad(&ciphertext.header_bytes(), tx_hash);
        let cipher = Aes256Gcm::new_from_slice(&data_key[..])
            .map_err(|e| TeeError::CryptoError(format!("failed to build content cipher: {e}")))?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&ciphertext.nonce),
                Payload {
                    msg: &ciphertext.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|e| {
                TeeError::CryptoError(format!(
                    "content decryption failed (AEAD tag rejected): {e}"
                ))
            })?;

        Ok(Zeroizing::new(plaintext))
    }
}

/// AAD for the payload AEAD layer: authenticated header || tx hash.
fn content_aad(header: &[u8; 10], tx_hash: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(10 + 32);
    aad.extend_from_slice(header);
    aad.extend_from_slice(tx_hash);
    aad
}

/// AAD for the committee KEM layer.
fn committee_kem_aad(epoch_id: u64, tx_hash: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(31 + 8 + 32);
    aad.extend_from_slice(b"neo-tee-mempool-kem-committee");
    aad.extend_from_slice(&epoch_id.to_le_bytes());
    aad.extend_from_slice(tx_hash);
    aad
}

/// AAD for the tee-sealed KEM layer (the monotonic counter is additionally
/// bound into the AEAD tag by `seal_data` itself).
fn sealed_kem_aad(tx_hash: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(29 + 32);
    aad.extend_from_slice(b"neo-tee-mempool-kem-sealed");
    aad.extend_from_slice(tx_hash);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn test_enclave() -> (tempfile::TempDir, Arc<TeeEnclave>) {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };
        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();
        (temp, enclave)
    }

    fn committee_kem(epoch_id: u64) -> KeyEncapsulation {
        KeyEncapsulation::CommitteeKey(CommitteeEpochKey::generate(epoch_id))
    }

    #[test]
    fn committee_mode_round_trip() {
        let tx_hash = [0x11u8; 32];
        let payload = b"committee-roundtrip-payload".to_vec();
        let kem = committee_kem(7);

        let sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        assert_eq!(sealed.version, CIPHERTEXT_VERSION);
        assert_eq!(sealed.mode, MODE_COMMITTEE_KEY);
        assert_eq!(sealed.epoch_id, 7);

        let opened = ContentCipher::open(&sealed, &tx_hash, &kem).unwrap();
        assert_eq!(&*opened, &payload[..]);
    }

    #[test]
    fn tee_sealed_mode_round_trip() {
        let (_temp, enclave) = test_enclave();
        let tx_hash = [0x22u8; 32];
        let payload = b"tee-sealed-roundtrip-payload".to_vec();
        let kem = KeyEncapsulation::TeeSealed(enclave);

        let sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        assert_eq!(sealed.mode, MODE_TEE_SEALED_KEY);
        assert_eq!(sealed.epoch_id, TEE_SEALED_EPOCH_ID);

        let opened = ContentCipher::open(&sealed, &tx_hash, &kem).unwrap();
        assert_eq!(&*opened, &payload[..]);
    }

    #[test]
    fn committee_wrong_key_rejected() {
        let tx_hash = [0x33u8; 32];
        let payload = b"secret-payload".to_vec();

        let sealed = ContentCipher::seal(&payload, &tx_hash, &committee_kem(7)).unwrap();
        // A different committee epoch key must not be able to decapsulate.
        let other = KeyEncapsulation::CommitteeKey(CommitteeEpochKey::generate(7));
        assert!(ContentCipher::open(&sealed, &tx_hash, &other).is_err());
    }

    #[test]
    fn committee_cross_epoch_rejected() {
        let tx_hash = [0x34u8; 32];
        let payload = b"cross-epoch-payload".to_vec();

        let sealed = ContentCipher::seal(&payload, &tx_hash, &committee_kem(7)).unwrap();
        // Same epoch id mismatch: configured epoch differs from header epoch.
        let other_epoch = KeyEncapsulation::CommitteeKey(CommitteeEpochKey::generate(8));
        assert!(ContentCipher::open(&sealed, &tx_hash, &other_epoch).is_err());
    }

    #[test]
    fn hash_substitution_rejected() {
        let tx_hash = [0x44u8; 32];
        let other_hash = [0x55u8; 32];
        let payload = b"bound-payload".to_vec();
        let kem = committee_kem(9);

        let sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        // The tx hash is authenticated: opening under a different hash fails.
        assert!(ContentCipher::open(&sealed, &other_hash, &kem).is_err());
    }

    #[test]
    fn tampered_payload_ciphertext_rejected() {
        let tx_hash = [0x66u8; 32];
        let payload = b"tamper-me".to_vec();
        let kem = committee_kem(11);

        let mut sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        // Flip a bit inside the payload ciphertext (the tag covers it).
        let last = sealed.ciphertext.len() - 1;
        sealed.ciphertext[last] ^= 0xFF;
        assert!(ContentCipher::open(&sealed, &tx_hash, &kem).is_err());
    }

    #[test]
    fn tampered_kem_material_rejected() {
        let tx_hash = [0x77u8; 32];
        let payload = b"tamper-kem".to_vec();
        let kem = committee_kem(12);

        let mut sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        // Flip a bit inside the committee KEM material.
        sealed.encapsulated_key[NONCE_LEN + 1] ^= 0xFF;
        assert!(ContentCipher::open(&sealed, &tx_hash, &kem).is_err());
    }

    #[test]
    fn encode_decode_round_trip_and_layout() {
        let tx_hash = [0x88u8; 32];
        let payload = b"envelope-layout".to_vec();
        let kem = committee_kem(3);

        let sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        let bytes = sealed.encode().unwrap();

        // Layout: version, mode, epoch id, kem length prefix.
        assert_eq!(bytes[0], CIPHERTEXT_VERSION);
        assert_eq!(bytes[1], MODE_COMMITTEE_KEY);
        assert_eq!(u64::from_le_bytes(bytes[2..10].try_into().unwrap()), 3);
        let kem_len = u16::from_le_bytes(bytes[10..12].try_into().unwrap()) as usize;
        assert_eq!(kem_len, sealed.encapsulated_key.len());
        // The payload must not appear anywhere in the envelope.
        assert!(
            !bytes
                .windows(payload.len())
                .any(|w| w == payload.as_slice())
        );

        let decoded = MempoolCiphertext::decode(&bytes).unwrap();
        assert_eq!(decoded, sealed);
    }

    #[test]
    fn decode_fails_closed_on_malformed_envelopes() {
        let tx_hash = [0x99u8; 32];
        let payload = b"decode-check".to_vec();
        let kem = committee_kem(1);
        let bytes = ContentCipher::seal(&payload, &tx_hash, &kem)
            .unwrap()
            .encode()
            .unwrap();

        // Truncated at every boundary must be rejected.
        for cut in [0usize, 1, 11, 12, bytes.len() - 1] {
            assert!(
                MempoolCiphertext::decode(&bytes[..cut]).is_err(),
                "cut {cut}"
            );
        }

        // Unknown version / mode are rejected fail-closed.
        let mut bad_version = bytes.clone();
        bad_version[0] = 0xFF;
        assert!(MempoolCiphertext::decode(&bad_version).is_err());

        let mut bad_mode = bytes.clone();
        bad_mode[1] = 0xEE;
        assert!(MempoolCiphertext::decode(&bad_mode).is_err());

        // Trailing garbage is rejected (strict length check).
        let mut trailing = bytes.clone();
        trailing.push(0x00);
        assert!(MempoolCiphertext::decode(&trailing).is_err());
    }

    #[test]
    fn tee_sealed_floor_rollback_rejects_decapsulation() {
        let (temp, enclave) = test_enclave();
        let tx_hash = [0xAAu8; 32];
        let payload = b"floor-protected-payload".to_vec();
        let kem = KeyEncapsulation::TeeSealed(enclave.clone());

        let sealed = ContentCipher::seal(&payload, &tx_hash, &kem).unwrap();
        // Sanity: decapsulation works before any tampering.
        assert!(ContentCipher::open(&sealed, &tx_hash, &kem).is_ok());

        // Roll the external floor witness back below the blob counter: the
        // blob is no longer covered by floor history -> fail closed.
        let floor_path = rollback_floor::floor_path(temp.path());
        rollback_floor::store(&floor_path, 0).unwrap();

        let error = ContentCipher::open(&sealed, &tx_hash, &kem)
            .expect_err("rolled-back floor must reject decapsulation");
        assert!(matches!(error, TeeError::RollbackProtection(_)));
    }

    #[test]
    fn debug_impl_redacts_committee_secret() {
        let key = CommitteeEpochKey::from_secret(4, [0x5Au8; 32]);
        let rendered = format!("{key:?}");
        assert!(rendered.contains("epoch_id: 4"));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("5a5a"));
    }
}

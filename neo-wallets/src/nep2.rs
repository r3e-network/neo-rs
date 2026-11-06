//! NEP-2 encrypted private key support.
//!
//! NEP-2 describes a passphrase-protected encoding of a Neo private key using
//! scrypt key derivation and AES-256-CBC encryption. This module is a faithful
//! port of the NEP-2 routines that lived in `neo-core`'s
//! `wallets::key_pair` (`encrypt_nep2` / `decrypt_nep2` /
//! `try_get_verification_script_for_key`), split out so the crypto can be
//! reused by both [`crate::key_pair::KeyPair`] and the NEP-6 wallet.
//!
//! # Preserved semantics
//!
//! * scrypt parameters `N = 16384`, `r = 8`, `p = 8`, 64-byte derived key.
//! * NEP-2 payload layout: `0x01 0x42 <flag=0xe0> <addressHash[4]> <encrypted[32]>`
//!   (39 bytes total), Base64 encoded when expressed as a string.
//! * The address hash is derived from the verification script produced by
//!   [`verification_script_for_private_key`], matching `neo-core` byte-for-byte.

use crate::wallet::{WalletError, WalletResult};
use aes::Aes256;
use base64::Engine;
use cbc::cipher::block_padding::NoPadding;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use cbc::{Decryptor, Encryptor};
use neo_crypto::{Crypto, ECC, ECCurve};
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

/// Length of a P-256 private key scalar in bytes.
pub const PRIVATE_KEY_SIZE: usize = 32;
/// Length of a raw NEP-2 encrypted payload in bytes.
pub const NEP2_PAYLOAD_SIZE: usize = 39;
/// NEP-2 magic prefix (`0x01 0x42`).
const NEP2_PREFIX: [u8; 2] = [0x01, 0x42];
/// NEP-2 flag byte.
const NEP2_FLAG: u8 = 0xe0;
/// `PUSHDATA1` opcode byte (matches `neo_vm::OpCode::PUSHDATA1`).
const OP_PUSHDATA1: u8 = 0x0C;
/// `SYSCALL` opcode byte (matches `neo_vm::OpCode::SYSCALL`).
const OP_SYSCALL: u8 = 0x41;
/// Length of the scrypt derived key (split into two 32-byte halves).
const DERIVED_KEY_SIZE: usize = 64;

/// Scrypt parameters used for NEP-2 encryption (C# `ScryptParameters`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScryptParameters {
    /// CPU/memory cost parameter (must be a power of two).
    pub n: u32,
    /// Block size parameter.
    pub r: u32,
    /// Parallelization parameter.
    pub p: u32,
}

impl ScryptParameters {
    /// Returns the default NEP-6 / NEP-2 scrypt parameters (`n=16384, r=8, p=8`).
    #[must_use]
    pub const fn default_nep6() -> Self {
        Self {
            n: 16384,
            r: 8,
            p: 8,
        }
    }

    /// Builds the `scrypt::Params` for these settings with a 64-byte output.
    fn to_scrypt_params(&self) -> WalletResult<scrypt::Params> {
        if self.n == 0 || !self.n.is_power_of_two() {
            return Err(WalletError::Scrypt(
                "scrypt N must be a non-zero power of two".to_string(),
            ));
        }
        scrypt::Params::new(
            self.n.trailing_zeros() as u8,
            self.r,
            self.p,
            DERIVED_KEY_SIZE,
        )
        .map_err(|e| WalletError::Scrypt(e.to_string()))
    }
}

impl Default for ScryptParameters {
    fn default() -> Self {
        Self::default_nep6()
    }
}

/// Encrypts a private key into a raw 39-byte NEP-2 payload using default parameters.
///
/// # Errors
///
/// Returns [`WalletError`] when the public key cannot be derived, scrypt fails,
/// or AES encryption fails.
pub fn encrypt(
    private_key: &[u8; PRIVATE_KEY_SIZE],
    password: &str,
    address_version: u8,
) -> WalletResult<Vec<u8>> {
    encrypt_with_params(
        private_key,
        password,
        address_version,
        &ScryptParameters::default_nep6(),
    )
}

/// Encrypts a private key into a raw NEP-2 payload using explicit scrypt parameters.
///
/// # Errors
///
/// Returns [`WalletError`] when the public key cannot be derived, scrypt fails,
/// or AES encryption fails.
pub fn encrypt_with_params(
    private_key: &[u8; PRIVATE_KEY_SIZE],
    password: &str,
    address_version: u8,
    params: &ScryptParameters,
) -> WalletResult<Vec<u8>> {
    // Address hash = first 4 bytes of hash256(address).
    let script_hash = UInt160::from_script(&verification_script_for_private_key(private_key)?);
    let address = crate::helper::Helper::to_address(&script_hash, address_version);
    let address_hash_full = Crypto::hash256(address.as_bytes());
    let mut address_hash = [0u8; 4];
    address_hash.copy_from_slice(&address_hash_full[0..4]);

    let scrypt_params = params.to_scrypt_params()?;
    let mut derived_key = Zeroizing::new([0u8; DERIVED_KEY_SIZE]);
    scrypt::scrypt(
        password.as_bytes(),
        &address_hash,
        &scrypt_params,
        derived_key.as_mut(),
    )
    .map_err(|e| WalletError::Scrypt(e.to_string()))?;

    let derived_half1 = &derived_key[0..PRIVATE_KEY_SIZE];
    let derived_half2 = &derived_key[32..DERIVED_KEY_SIZE];

    // XOR the private key with derived_half1.
    let mut xor_key = Zeroizing::new([0u8; PRIVATE_KEY_SIZE]);
    for i in 0..PRIVATE_KEY_SIZE {
        xor_key[i] = private_key[i] ^ derived_half1[i];
    }

    let cipher = Encryptor::<Aes256>::new_from_slices(derived_half2, &[0u8; 16])
        .map_err(|e| WalletError::Aes(e.to_string()))?;
    let mut buffer = Zeroizing::new(xor_key.to_vec());
    let encrypted = cipher
        .encrypt_padded_mut::<NoPadding>(buffer.as_mut_slice(), PRIVATE_KEY_SIZE)
        .map_err(|e| WalletError::Aes(e.to_string()))?;
    let encrypted = encrypted.to_vec();

    let mut result = Vec::with_capacity(NEP2_PAYLOAD_SIZE);
    result.extend_from_slice(&NEP2_PREFIX);
    result.push(NEP2_FLAG);
    result.extend_from_slice(&address_hash);
    result.extend_from_slice(&encrypted);
    Ok(result)
}

/// Decrypts a raw 39-byte NEP-2 payload into a private key using default parameters.
///
/// # Errors
///
/// Returns [`WalletError::InvalidNep2Key`] for malformed payloads,
/// [`WalletError::InvalidPassword`] when the password does not match, and
/// scrypt/AES errors otherwise.
pub fn decrypt(
    encrypted_key: &[u8],
    password: &str,
    address_version: u8,
) -> WalletResult<[u8; PRIVATE_KEY_SIZE]> {
    decrypt_with_params(
        encrypted_key,
        password,
        address_version,
        &ScryptParameters::default_nep6(),
    )
}

/// Decrypts a raw NEP-2 payload into a private key using explicit scrypt parameters.
///
/// # Errors
///
/// Returns [`WalletError::InvalidNep2Key`] for malformed payloads,
/// [`WalletError::InvalidPassword`] when the password does not match, and
/// scrypt/AES errors otherwise.
pub fn decrypt_with_params(
    encrypted_key: &[u8],
    password: &str,
    address_version: u8,
    params: &ScryptParameters,
) -> WalletResult<[u8; PRIVATE_KEY_SIZE]> {
    if encrypted_key.len() != NEP2_PAYLOAD_SIZE {
        return Err(WalletError::InvalidNep2Key(
            "invalid NEP-2 key length".to_string(),
        ));
    }
    if encrypted_key[0..2] != NEP2_PREFIX {
        return Err(WalletError::InvalidNep2Key(
            "invalid NEP-2 key prefix".to_string(),
        ));
    }

    let address_hash = &encrypted_key[3..7];
    let encrypted_data = &encrypted_key[7..NEP2_PAYLOAD_SIZE];

    let scrypt_params = params.to_scrypt_params()?;
    let mut derived_key = Zeroizing::new([0u8; DERIVED_KEY_SIZE]);
    scrypt::scrypt(
        password.as_bytes(),
        address_hash,
        &scrypt_params,
        derived_key.as_mut(),
    )
    .map_err(|e| WalletError::Scrypt(e.to_string()))?;

    let derived_half1 = &derived_key[0..PRIVATE_KEY_SIZE];
    let derived_half2 = &derived_key[32..DERIVED_KEY_SIZE];

    let cipher = Decryptor::<Aes256>::new_from_slices(derived_half2, &[0u8; 16])
        .map_err(|e| WalletError::Aes(e.to_string()))?;
    let mut buffer = Zeroizing::new(encrypted_data.to_vec());
    let decrypted = cipher
        .decrypt_padded_mut::<NoPadding>(buffer.as_mut_slice())
        .map_err(|e| WalletError::Aes(e.to_string()))?;

    let mut private_key = [0u8; PRIVATE_KEY_SIZE];
    for i in 0..PRIVATE_KEY_SIZE {
        private_key[i] = decrypted[i] ^ derived_half1[i];
    }

    // Verify the password by recomputing the address hash.
    let verification_script = verification_script_for_private_key(&private_key)?;
    let script_hash = UInt160::from_script(&verification_script);
    let address = crate::helper::Helper::to_address(&script_hash, address_version);
    let computed_hash_full = Crypto::hash256(address.as_bytes());
    let computed_hash = &computed_hash_full[0..4];

    // Constant-time comparison to prevent a timing oracle on password verification.
    if !bool::from(computed_hash.ct_eq(address_hash)) {
        private_key.zeroize();
        return Err(WalletError::InvalidPassword);
    }

    Ok(private_key)
}

/// Encrypts a private key and returns the Base64-encoded NEP-2 string.
///
/// # Errors
///
/// Propagates any error from [`encrypt_with_params`].
pub fn encrypt_to_string(
    private_key: &[u8; PRIVATE_KEY_SIZE],
    password: &str,
    address_version: u8,
    params: &ScryptParameters,
) -> WalletResult<String> {
    let encrypted = encrypt_with_params(private_key, password, address_version, params)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(encrypted))
}

/// Decrypts a Base64-encoded NEP-2 string into a private key.
///
/// # Errors
///
/// Returns [`WalletError::InvalidNep2Key`] when the input is not valid Base64,
/// and propagates any error from [`decrypt_with_params`] otherwise.
pub fn decrypt_from_string(
    encrypted_key: &str,
    password: &str,
    address_version: u8,
    params: &ScryptParameters,
) -> WalletResult<[u8; PRIVATE_KEY_SIZE]> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encrypted_key.as_bytes())
        .map_err(|_| WalletError::InvalidNep2Key("invalid NEP-2 encrypted key".to_string()))?;
    decrypt_with_params(&decoded, password, address_version, params)
}

/// Builds the verification script used to derive the NEP-2 address hash.
///
/// NOTE: This intentionally reproduces `neo-core`'s
/// `KeyPair::try_get_verification_script_for_key`, which appends the literal
/// ASCII bytes `System.Crypto.CheckWitness` rather than the 4-byte
/// `System.Crypto.CheckSig` interop hash used by
/// [`crate::helper::Helper::signature_redeem_script`]. Preserving this keeps
/// NEP-2 payloads byte-compatible with keys produced by the existing
/// `neo-core` wallet. See the crate handoff report (task 89 alignment) for the
/// recommendation to reconcile this with the NEP-2 standard.
fn verification_script_for_private_key(
    private_key: &[u8; PRIVATE_KEY_SIZE],
) -> WalletResult<Vec<u8>> {
    let public_point = ECC::generate_public_key(private_key, ECCurve::secp256r1())
        .map_err(|e| WalletError::Crypto(format!("Failed to generate public key: {e}")))?;
    let compressed = ECC::compress_public_key(&public_point)
        .map_err(|e| WalletError::Crypto(format!("Failed to compress public key: {e}")))?;

    let mut script = Vec::with_capacity(compressed.len() + 28);
    script.push(OP_PUSHDATA1);
    script.push(compressed.len() as u8);
    script.extend_from_slice(&compressed);
    script.push(OP_SYSCALL);
    script.extend_from_slice(b"System.Crypto.CheckWitness");
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS_VERSION: u8 = 0x35;

    fn sample_key() -> [u8; PRIVATE_KEY_SIZE] {
        let mut key = [0u8; PRIVATE_KEY_SIZE];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        key
    }

    #[test]
    fn nep2_round_trip_default_params() {
        let key = sample_key();
        let encrypted = encrypt(&key, "correct horse", ADDRESS_VERSION).unwrap();
        assert_eq!(encrypted.len(), NEP2_PAYLOAD_SIZE);
        assert_eq!(&encrypted[0..2], &NEP2_PREFIX);
        assert_eq!(encrypted[2], NEP2_FLAG);
        let decrypted = decrypt(&encrypted, "correct horse", ADDRESS_VERSION).unwrap();
        assert_eq!(decrypted, key);
    }

    #[test]
    fn nep2_string_round_trip() {
        let key = sample_key();
        let params = ScryptParameters::default_nep6();
        let encoded = encrypt_to_string(&key, "pw", ADDRESS_VERSION, &params).unwrap();
        let decrypted = decrypt_from_string(&encoded, "pw", ADDRESS_VERSION, &params).unwrap();
        assert_eq!(decrypted, key);
    }

    #[test]
    fn nep2_wrong_password_is_rejected() {
        let key = sample_key();
        let encrypted = encrypt(&key, "right", ADDRESS_VERSION).unwrap();
        assert!(matches!(
            decrypt(&encrypted, "wrong", ADDRESS_VERSION),
            Err(WalletError::InvalidPassword)
        ));
    }

    #[test]
    fn nep2_rejects_malformed_payload() {
        assert!(matches!(
            decrypt(&[0u8; 10], "pw", ADDRESS_VERSION),
            Err(WalletError::InvalidNep2Key(_))
        ));
        let mut bad = encrypt(&sample_key(), "pw", ADDRESS_VERSION).unwrap();
        bad[0] = 0x00;
        assert!(matches!(
            decrypt(&bad, "pw", ADDRESS_VERSION),
            Err(WalletError::InvalidNep2Key(_))
        ));
    }

    #[test]
    fn scrypt_parameters_default_to_nep2_values() {
        let params = ScryptParameters::default();
        assert_eq!((params.n, params.r, params.p), (16384, 8, 8));
    }
}

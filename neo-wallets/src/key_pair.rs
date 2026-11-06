//! Key pair implementation for Neo wallets.
//!
//! Faithful port of `neo-core`'s `wallets::key_pair::KeyPair`, re-based onto the
//! standalone dependency set (`neo-crypto` for P-256 primitives, `neo-primitives`
//! for `UInt160` / Base58Check) with NEP-2 delegated to [`crate::nep2`].
//!
//! # Security Note
//!
//! The `Debug` implementation intentionally hides the private key to prevent
//! accidental exposure in logs. Use [`KeyPair::private_key`] for explicit access.

use crate::helper::Helper;
use crate::nep2::{self, PRIVATE_KEY_SIZE, ScryptParameters};
use crate::wallet::{WalletError, WalletResult};
use neo_crypto::{ECC, ECCurve, ECDsa, ECPoint, Secp256r1Crypto};
use neo_primitives::UInt160;
use neo_primitives::base58_check;
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

/// WIF version byte for a mainnet private key.
const WIF_VERSION: u8 = 0x80;
/// WIF compressed-flag suffix byte.
const WIF_COMPRESSED_FLAG: u8 = 0x01;
/// Decoded WIF payload length: version(1) + key(32) + flag(1).
const WIF_DECODED_SIZE: usize = 34;

/// A cryptographic key pair for Neo accounts.
///
/// Matches the C# `KeyPair` class functionality.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct KeyPair {
    private_key: [u8; PRIVATE_KEY_SIZE],
    public_key: Vec<u8>,
    compressed_public_key: Vec<u8>,
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPair")
            .field("private_key", &"[REDACTED]")
            .field("public_key", &hex::encode(&self.public_key))
            .field(
                "compressed_public_key",
                &hex::encode(&self.compressed_public_key),
            )
            .finish()
    }
}

impl fmt::Display for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.compressed_public_key))
    }
}

impl KeyPair {
    /// Creates a new random key pair using the shared P-256 generator.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] if the generated scalar cannot be turned into a key pair.
    pub fn generate() -> WalletResult<Self> {
        let private_key = Zeroizing::new(Secp256r1Crypto::generate_private_key());
        Self::from_private_key(private_key.as_ref())
    }

    /// Creates a key pair from a raw private key buffer.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidPrivateKey`] when the slice is not 32 bytes.
    pub fn new(private_key: Vec<u8>) -> WalletResult<Self> {
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a private key slice.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidPrivateKey`] when the slice is not 32 bytes,
    /// or [`WalletError::Crypto`] when the public key cannot be derived.
    pub fn from_private_key(private_key: &[u8]) -> WalletResult<Self> {
        if private_key.len() != PRIVATE_KEY_SIZE {
            return Err(WalletError::InvalidPrivateKey(
                "private key must be 32 bytes".to_string(),
            ));
        }

        let mut key_bytes = [0u8; PRIVATE_KEY_SIZE];
        key_bytes.copy_from_slice(private_key);

        let public_point = ECC::generate_public_key(&key_bytes, ECCurve::secp256r1())
            .map_err(|e| WalletError::Crypto(format!("Failed to derive public key: {e}")))?;
        let public_key = public_point.to_bytes();
        let compressed_public_key = ECC::compress_public_key(&public_point)
            .map_err(|e| WalletError::Crypto(format!("Failed to compress public key: {e}")))?;

        Ok(Self {
            private_key: key_bytes,
            public_key,
            compressed_public_key,
        })
    }

    /// Creates a key pair from a WIF (Wallet Import Format) string.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidWif`] for malformed WIF input.
    pub fn from_wif(wif: &str) -> WalletResult<Self> {
        let private_key = Self::decode_wif(wif)?;
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a Base64-encoded NEP-2 encrypted key (bytes).
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidNep2Key`] when the bytes are not valid
    /// Base64, or [`WalletError::InvalidPassword`] when decryption fails.
    pub fn from_nep2(
        encrypted_key: &[u8],
        password: &str,
        address_version: u8,
    ) -> WalletResult<Self> {
        let text = std::str::from_utf8(encrypted_key)
            .map_err(|_| WalletError::InvalidNep2Key("invalid NEP-2 encrypted key".to_string()))?;
        Self::from_nep2_string(text, password, address_version)
    }

    /// Creates a key pair from a Base64-encoded NEP-2 encrypted key string.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidNep2Key`] or [`WalletError::InvalidPassword`].
    pub fn from_nep2_string(
        encrypted_key: &str,
        password: &str,
        address_version: u8,
    ) -> WalletResult<Self> {
        let private_key = nep2::decrypt_from_string(
            encrypted_key,
            password,
            address_version,
            &ScryptParameters::default_nep6(),
        )?;
        Self::from_private_key(&private_key)
    }

    /// Gets the private key.
    #[must_use]
    pub const fn private_key(&self) -> &[u8; PRIVATE_KEY_SIZE] {
        &self.private_key
    }

    /// Gets the public key (uncompressed).
    #[must_use]
    pub fn public_key(&self) -> Vec<u8> {
        self.public_key.clone()
    }

    /// Gets the compressed public key.
    #[must_use]
    pub fn compressed_public_key(&self) -> Vec<u8> {
        self.compressed_public_key.clone()
    }

    /// Gets the public key as an [`ECPoint`].
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Crypto`] when the compressed key cannot be decoded.
    pub fn get_public_key_point(&self) -> WalletResult<ECPoint> {
        ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &self.compressed_public_key)
            .map_err(|e| WalletError::Crypto(format!("Failed to create ECPoint: {e}")))
    }

    /// Gets the script hash for this key pair (C# `KeyPair.PublicKeyHash`).
    #[must_use]
    pub fn get_script_hash(&self) -> UInt160 {
        UInt160::from_script(&self.get_verification_script())
    }

    /// Gets the signature verification script for this key pair.
    #[must_use]
    pub fn get_verification_script(&self) -> Vec<u8> {
        Helper::signature_redeem_script(&self.compressed_public_key)
    }

    /// Signs data with this key pair (P-256 ECDSA, 64-byte signature).
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when signing fails.
    pub fn sign(&self, data: &[u8]) -> WalletResult<Vec<u8>> {
        ECDsa::sign(data, &self.private_key, ECCurve::secp256r1())
            .map(|sig| sig.to_vec())
            .map_err(|e| WalletError::SigningFailed(format!("Signing failed: {e}")))
    }

    /// Verifies a signature against data using the uncompressed public key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::SigningFailed`] when verification cannot be performed.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> WalletResult<bool> {
        ECDsa::verify(data, signature, &self.public_key, ECCurve::secp256r1())
            .map_err(|e| WalletError::SigningFailed(format!("Verification failed: {e}")))
    }

    /// Exports the key pair to WIF format.
    #[must_use]
    pub fn to_wif(&self) -> String {
        Self::encode_wif(&self.private_key)
    }

    /// Exports the key pair to a Base64-encoded NEP-2 string.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`nep2::encrypt_to_string`].
    pub fn to_nep2(&self, password: &str, address_version: u8) -> WalletResult<String> {
        nep2::encrypt_to_string(
            &self.private_key,
            password,
            address_version,
            &ScryptParameters::default_nep6(),
        )
    }

    /// Decodes a WIF string to a private key.
    fn decode_wif(wif: &str) -> WalletResult<[u8; PRIVATE_KEY_SIZE]> {
        let data = base58_check::decode_check(wif)
            .map_err(|_| WalletError::InvalidWif("invalid WIF format".to_string()))?;

        if data.len() != WIF_DECODED_SIZE {
            return Err(WalletError::InvalidWif("invalid WIF length".to_string()));
        }
        if data[0] != WIF_VERSION {
            return Err(WalletError::InvalidWif(
                "invalid WIF version byte".to_string(),
            ));
        }
        if data[WIF_DECODED_SIZE - 1] != WIF_COMPRESSED_FLAG {
            return Err(WalletError::InvalidWif(
                "invalid WIF compressed flag".to_string(),
            ));
        }

        let mut private_key = [0u8; PRIVATE_KEY_SIZE];
        private_key.copy_from_slice(&data[1..33]);
        Ok(private_key)
    }

    /// Encodes a private key to WIF format.
    fn encode_wif(private_key: &[u8; PRIVATE_KEY_SIZE]) -> String {
        let mut data = Vec::with_capacity(WIF_DECODED_SIZE);
        data.push(WIF_VERSION);
        data.extend_from_slice(private_key);
        data.push(WIF_COMPRESSED_FLAG);
        base58_check::encode_check(&data)
    }
}

impl ConstantTimeEq for KeyPair {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.private_key.ct_eq(&other.private_key)
    }
}

impl PartialEq for KeyPair {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).into()
    }
}

impl Eq for KeyPair {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_pair_generation() {
        let key_pair = KeyPair::generate().unwrap();
        assert_eq!(key_pair.private_key().len(), PRIVATE_KEY_SIZE);
        assert!(!key_pair.public_key().is_empty());
        assert!(!key_pair.compressed_public_key().is_empty());
    }

    #[test]
    fn test_wif_round_trip() {
        let key_pair = KeyPair::generate().unwrap();
        let wif = key_pair.to_wif();
        let restored = KeyPair::from_wif(&wif).unwrap();
        assert_eq!(key_pair.private_key(), restored.private_key());
    }

    #[test]
    fn test_sign_verify() {
        let key_pair = KeyPair::generate().unwrap();
        let data = b"test data";
        let signature = key_pair.sign(data).unwrap();
        assert!(key_pair.verify(data, &signature).unwrap());
    }

    #[test]
    fn test_nep2_round_trip() {
        let key_pair = KeyPair::generate().unwrap();
        let nep2 = key_pair.to_nep2("s3cr3t-passphrase", 0x35).unwrap();
        let restored = KeyPair::from_nep2_string(&nep2, "s3cr3t-passphrase", 0x35).unwrap();
        assert_eq!(key_pair.private_key(), restored.private_key());
        assert!(KeyPair::from_nep2_string(&nep2, "wrong-passphrase", 0x35).is_err());
    }

    #[test]
    fn test_script_hash_matches_verification_script() {
        let key_pair = KeyPair::generate().unwrap();
        let script = key_pair.get_verification_script();
        assert_eq!(script.len(), 40);
        assert_eq!(key_pair.get_script_hash(), UInt160::from_script(&script));
    }
}

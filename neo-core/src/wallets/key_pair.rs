//! Key pair implementation for Neo wallets.
//!
//! This module provides cryptographic key pair functionality,
//! converted from the C# Neo KeyPair class (@neo-sharp/src/Neo/Wallets/KeyPair.cs).

use crate::cryptography::{ECCurve, ECDsa, ECC};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::neo_config::HASH_SIZE;
use crate::smart_contract::helper::Helper;
use crate::wallets::helper::Helper as WalletHelper;
use crate::UInt160;
use aes::Aes256;
use base64::Engine;
use cbc::{
    cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Decryptor, Encryptor,
};
use rand::RngCore;
use scrypt::Params;
use std::fmt;
use zeroize::{Zeroize, Zeroizing};

/// A cryptographic key pair for Neo accounts.
/// This matches the C# KeyPair class functionality.
#[derive(Debug, Clone, Zeroize)]
#[zeroize(drop)]
pub struct KeyPair {
    private_key: [u8; HASH_SIZE],
    public_key: Vec<u8>,
    compressed_public_key: Vec<u8>,
}

impl KeyPair {
    /// Creates a new random key pair.
    pub fn generate() -> Result<Self> {
        let mut private_key = [0u8; HASH_SIZE];
        rand::thread_rng().fill_bytes(&mut private_key);
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a raw private key buffer.
    pub fn new(private_key: Vec<u8>) -> Result<Self> {
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a private key.
    pub fn from_private_key(private_key: &[u8]) -> Result<Self> {
        if private_key.len() != HASH_SIZE {
            return Err(Error::InvalidPrivateKey);
        }

        let mut key_bytes = [0u8; HASH_SIZE];
        key_bytes.copy_from_slice(private_key);

        // Generate public key from private key
        let public_point =
            ECC::generate_public_key(&key_bytes, ECCurve::secp256r1()).map_err(|e| {
                Error::Other {
                    message: format!("Failed to derive public key: {}", e),
                }
            })?;
        let public_key = public_point.to_bytes();
        let compressed_public_key =
            ECC::compress_public_key(&public_point).map_err(|e| Error::Other {
                message: format!("Failed to compress public key: {}", e),
            })?;

        Ok(Self {
            private_key: key_bytes,
            public_key,
            compressed_public_key,
        })
    }

    /// Creates a key pair from a WIF (Wallet Import Format) string.
    pub fn from_wif(wif: &str) -> Result<Self> {
        let private_key = Self::decode_wif(wif)?;
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a NEP-2 encrypted private key.
    /// The encrypted_key should be base64-encoded NEP-2 data.
    pub fn from_nep2(encrypted_key: &[u8], password: &str, address_version: u8) -> Result<Self> {
        // First try to decode as base64
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encrypted_key)
            .map_err(|_| Error::InvalidNep2Key)?;

        let private_key = Self::decrypt_nep2(&decoded, password, address_version)?;
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a NEP-2 encrypted private key string.
    /// The encrypted_key should be a base64-encoded NEP-2 string.
    pub fn from_nep2_string(
        encrypted_key: &str,
        password: &str,
        address_version: u8,
    ) -> Result<Self> {
        Self::from_nep2(encrypted_key.as_bytes(), password, address_version)
    }

    /// Gets the private key.
    pub fn private_key(&self) -> [u8; HASH_SIZE] {
        self.private_key
    }

    /// Gets the public key (uncompressed).
    pub fn public_key(&self) -> Vec<u8> {
        self.public_key.clone()
    }

    /// Gets the compressed public key.
    pub fn compressed_public_key(&self) -> Vec<u8> {
        self.compressed_public_key.clone()
    }

    /// Gets the public key as an ECPoint.
    pub fn get_public_key_point(&self) -> Result<crate::neo_cryptography::ECPoint> {
        crate::neo_cryptography::ECPoint::decode_compressed(&self.compressed_public_key).map_err(
            |e| Error::Other {
                message: format!("Failed to create ECPoint: {}", e),
            },
        )
    }

    /// Gets the script hash for this key pair.
    /// This matches the C# KeyPair.PublicKeyHash property.
    pub fn get_script_hash(&self) -> UInt160 {
        UInt160::from_script(&self.get_verification_script())
    }

    /// Gets the verification script for this key pair.
    pub fn get_verification_script(&self) -> Vec<u8> {
        Helper::signature_redeem_script(&self.compressed_public_key)
    }

    /// Signs data with this key pair.
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        ECDsa::sign(data, &self.private_key, ECCurve::secp256r1())
            .map(|sig| sig.to_vec())
            .map_err(|e| Error::Other {
                message: format!("Signing failed: {}", e),
            })
    }

    /// Verifies a signature against data.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool> {
        ECDsa::verify(data, signature, &self.public_key, ECCurve::secp256r1()).map_err(|e| {
            Error::Other {
                message: format!("Verification failed: {}", e),
            }
        })
    }

    /// Exports the key pair to WIF format.
    pub fn to_wif(&self) -> String {
        Self::encode_wif(&self.private_key)
    }

    /// Exports the key pair to NEP-2 format.
    pub fn to_nep2(&self, password: &str, address_version: u8) -> Result<String> {
        let encrypted = Self::encrypt_nep2(&self.private_key, password, address_version)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(encrypted))
    }

    /// Decodes a WIF string to a private key.
    fn decode_wif(wif: &str) -> Result<[u8; HASH_SIZE]> {
        let decoded = bs58::decode(wif)
            .into_vec()
            .map_err(|e| Error::Base58Decode {
                message: e.to_string(),
            })?;

        // Verify checksum manually
        if decoded.len() < 4 {
            return Err(Error::InvalidWif);
        }

        let (data, checksum) = decoded.split_at(decoded.len() - 4);
        let computed_checksum = &crate::neo_cryptography::hash::hash256(data)[0..4];
        if checksum != computed_checksum {
            return Err(Error::InvalidWif);
        }

        if decoded.len() != 38 {
            return Err(Error::InvalidWif);
        }

        // Check version byte
        if data[0] != 0x80 {
            return Err(Error::InvalidWif);
        }

        // Check compressed flag
        if data[33] != 0x01 {
            return Err(Error::InvalidWif);
        }

        let mut private_key = [0u8; HASH_SIZE];
        private_key.copy_from_slice(&data[1..33]);
        Ok(private_key)
    }

    /// Encodes a private key to WIF format.
    fn encode_wif(private_key: &[u8; HASH_SIZE]) -> String {
        let mut data = Vec::with_capacity(37);
        data.push(0x80); // Version byte for mainnet
        data.extend_from_slice(private_key);
        data.push(0x01); // Compressed flag

        // Add checksum manually
        let checksum = &crate::neo_cryptography::hash::hash256(&data)[0..4];
        data.extend_from_slice(checksum);

        bs58::encode(data).into_string()
    }

    /// Encrypts a private key using NEP-2 standard.
    fn encrypt_nep2(
        private_key: &[u8; HASH_SIZE],
        password: &str,
        address_version: u8,
    ) -> Result<Vec<u8>> {
        // NEP-2 parameters
        let n = 16384; // CPU cost
        let r = 8; // Memory cost
        let p = 8; // Parallelization

        // Generate address hash
        let script_hash = UInt160::from_script(&Self::try_get_verification_script_for_key(private_key)?);
        let address = WalletHelper::to_address(&script_hash, address_version);
        let address_hash_full = crate::neo_cryptography::hash::hash256(address.as_bytes());
        let mut address_hash = [0u8; 4];
        address_hash.copy_from_slice(&address_hash_full[0..4]);

        // Derive key using scrypt
        let n: u32 = n;
        let params =
            Params::new(n.trailing_zeros() as u8, r, p, 64).map_err(|e| Error::Scrypt {
                message: e.to_string(),
            })?;

        // Use Zeroizing wrapper to ensure sensitive data is cleared on drop
        let mut derived_key = Zeroizing::new([0u8; 64]);
        scrypt::scrypt(
            password.as_bytes(),
            &address_hash,
            &params,
            derived_key.as_mut(),
        )
        .map_err(|e| Error::Scrypt {
            message: e.to_string(),
        })?;

        // Split derived key
        let derived_half1 = &derived_key[0..HASH_SIZE];
        let derived_half2 = &derived_key[32..64];

        // XOR private key with derived_half1 (use Zeroizing for sensitive intermediate)
        let mut xor_key = Zeroizing::new([0u8; HASH_SIZE]);
        for i in 0..HASH_SIZE {
            xor_key[i] = private_key[i] ^ derived_half1[i];
        }

        let cipher =
            Encryptor::<Aes256>::new_from_slices(derived_half2, &[0u8; 16]).map_err(|e| {
                Error::Aes {
                    message: e.to_string(),
                }
            })?;
        let mut buffer = Zeroizing::new(xor_key.to_vec());
        buffer.resize(HASH_SIZE, 0); // Ensure exactly HASH_SIZE bytes
        let encrypted = cipher
            .encrypt_padded_mut::<cbc::cipher::block_padding::NoPadding>(buffer.as_mut_slice(), HASH_SIZE)
            .map_err(|e| Error::Aes {
                message: e.to_string(),
            })?;
        let encrypted = encrypted.to_vec();

        let mut result = Vec::with_capacity(39);
        result.extend_from_slice(b"\x01\x42"); // NEP-2 prefix
        result.push(0xe0); // Flags
        result.extend_from_slice(&address_hash);
        result.extend_from_slice(&encrypted);

        Ok(result)
    }

    /// Decrypts a NEP-2 encrypted private key.
    fn decrypt_nep2(
        encrypted_key: &[u8],
        password: &str,
        address_version: u8,
    ) -> Result<[u8; HASH_SIZE]> {
        if encrypted_key.len() != 39 {
            return Err(Error::InvalidNep2Key);
        }

        if &encrypted_key[0..2] != b"\x01\x42" {
            return Err(Error::InvalidNep2Key);
        }

        let _flags = encrypted_key[2];
        let address_hash = &encrypted_key[3..7];
        let encrypted_data = &encrypted_key[7..39];

        // NEP-2 parameters
        let n = 16384;
        let r = 8;
        let p = 8;

        // Derive key using scrypt (use Zeroizing for sensitive data)
        let n: u32 = n;
        let params =
            Params::new(n.trailing_zeros() as u8, r, p, 64).map_err(|e| Error::Scrypt {
                message: e.to_string(),
            })?;

        let mut derived_key = Zeroizing::new([0u8; 64]);
        scrypt::scrypt(password.as_bytes(), address_hash, &params, derived_key.as_mut()).map_err(
            |e| Error::Scrypt {
                message: e.to_string(),
            },
        )?;

        let derived_half1 = &derived_key[0..HASH_SIZE];
        let derived_half2 = &derived_key[32..64];

        let cipher =
            Decryptor::<Aes256>::new_from_slices(derived_half2, &[0u8; 16]).map_err(|e| {
                Error::Aes {
                    message: e.to_string(),
                }
            })?;
        let mut buffer = Zeroizing::new(encrypted_data.to_vec());
        let decrypted = cipher
            .decrypt_padded_mut::<cbc::cipher::block_padding::NoPadding>(buffer.as_mut_slice())
            .map_err(|e| Error::Aes {
                message: e.to_string(),
            })?;

        // XOR with derived_half1 to get private key
        let mut private_key = [0u8; HASH_SIZE];
        for i in 0..HASH_SIZE {
            private_key[i] = decrypted[i] ^ derived_half1[i];
        }

        // Verify by checking address hash
        let verification_script = Self::try_get_verification_script_for_key(&private_key)?;
        let script_hash = UInt160::from_script(&verification_script);
        let address = WalletHelper::to_address(&script_hash, address_version);
        let computed_hash_full = crate::neo_cryptography::hash::hash256(address.as_bytes());
        let computed_hash = &computed_hash_full[0..4];

        if computed_hash != address_hash {
            // Zeroize private key before returning error
            private_key.zeroize();
            return Err(Error::InvalidPassword);
        }

        Ok(private_key)
    }

    /// Gets verification script for a private key (helper function).
    /// Returns Result instead of panicking on failure.
    fn try_get_verification_script_for_key(private_key: &[u8; HASH_SIZE]) -> Result<Vec<u8>> {
        let public_point =
            ECC::generate_public_key(private_key, ECCurve::secp256r1()).map_err(|e| {
                Error::Other {
                    message: format!("Failed to generate public key: {}", e),
                }
            })?;
        let compressed = ECC::compress_public_key(&public_point).map_err(|e| Error::Other {
            message: format!("Failed to compress public key: {}", e),
        })?;

        let mut script = Vec::new();
        script.push(0x0c); // PUSHDATA1
        script.push(compressed.len() as u8);
        script.extend_from_slice(&compressed);
        script.push(0x41); // SYSCALL
        script.extend_from_slice(b"System.Crypto.CheckWitness");
        Ok(script)
    }
}

impl fmt::Display for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.compressed_public_key))
    }
}

impl PartialEq for KeyPair {
    fn eq(&self, other: &Self) -> bool {
        self.private_key == other.private_key
    }
}

impl Eq for KeyPair {}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::neo_config::HASH_SIZE;

    #[test]
    fn test_key_pair_generation() {
        let key_pair = KeyPair::generate().unwrap();
        assert_eq!(key_pair.private_key().len(), HASH_SIZE);
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
}

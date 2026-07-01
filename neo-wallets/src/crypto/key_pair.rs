//! Key pair implementation for Neo wallets.
//!
//! This module provides cryptographic key pair functionality,
//! converted from the C# Neo KeyPair class (@neo-sharp/src/Neo/Wallets/KeyPair.cs).

use crate::wallet_helper::WalletAddress;
use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use neo_crypto::{CryptoError, ECC, ECCurve, ECDsa, Secp256r1Crypto, base58};
use neo_error::{CoreError, CoreResult};
use neo_execution::Helper;
use neo_primitives::HASH_SIZE;
use neo_primitives::UInt160;
use scrypt::Params;
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

/// A cryptographic key pair for Neo accounts.
/// This matches the C# KeyPair class functionality.
///
/// # Security Note
/// The `Debug` implementation intentionally hides the private key to prevent
/// accidental exposure in logs. Use [`KeyPair::private_key()`] for explicit access.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct KeyPair {
    private_key: [u8; HASH_SIZE],
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

impl KeyPair {
    /// Creates a new random key pair.
    ///
    /// Uses the shared P-256 key generator from `neo-crypto`.
    pub fn generate() -> CoreResult<Self> {
        let private_key = Zeroizing::new(Secp256r1Crypto::generate_private_key());
        Self::from_private_key(private_key.as_ref())
    }

    /// Creates a key pair from a raw private key buffer.
    pub fn new(private_key: Vec<u8>) -> CoreResult<Self> {
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a private key.
    pub fn from_private_key(private_key: &[u8]) -> CoreResult<Self> {
        if private_key.len() != HASH_SIZE {
            return Err(CoreError::invalid_private_key(
                "private key must be 32 bytes",
            ));
        }

        let mut key_bytes = [0u8; HASH_SIZE];
        key_bytes.copy_from_slice(private_key);

        // Generate public key from private key
        let public_point =
            ECC::generate_public_key(&key_bytes, ECCurve::secp256r1()).map_err(|e| {
                CoreError::InvalidOperation {
                    message: format!("Failed to derive public key: {}", e),
                }
            })?;
        let public_key = public_point.to_bytes();
        let compressed_public_key =
            ECC::compress_public_key(&public_point).map_err(|e| CoreError::InvalidOperation {
                message: format!("Failed to compress public key: {}", e),
            })?;

        Ok(Self {
            private_key: key_bytes,
            public_key,
            compressed_public_key,
        })
    }

    /// Creates a key pair from a WIF (Wallet Import Format) string.
    pub fn from_wif(wif: &str) -> CoreResult<Self> {
        let private_key = Self::decode_wif(wif)?;
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a NEP-2 encrypted private key.
    /// The encrypted_key should be the Base58Check-encoded NEP-2 "6P..." string.
    pub fn from_nep2(
        encrypted_key: &[u8],
        password: &str,
        address_version: u8,
    ) -> CoreResult<Self> {
        // NEP-2 strings are Base58Check-encoded (standard "6P..." form), matching
        // C# Wallet.GetPrivateKeyFromNEP2 -> Base58.Base58CheckDecode.
        let encrypted_str = std::str::from_utf8(encrypted_key)
            .map_err(|_| CoreError::invalid_nep2_key("invalid NEP-2 encrypted key"))?;
        let decoded = base58::decode_check(encrypted_str)
            .map_err(|_| CoreError::invalid_nep2_key("invalid NEP-2 encrypted key"))?;

        let private_key = Self::decrypt_nep2(&decoded, password, address_version)?;
        Self::from_private_key(&private_key)
    }

    /// Creates a key pair from a NEP-2 encrypted private key string.
    /// The encrypted_key should be the Base58Check-encoded NEP-2 "6P..." string.
    pub fn from_nep2_string(
        encrypted_key: &str,
        password: &str,
        address_version: u8,
    ) -> CoreResult<Self> {
        Self::from_nep2(encrypted_key.as_bytes(), password, address_version)
    }

    /// Gets the private key.
    pub fn private_key(&self) -> &[u8; HASH_SIZE] {
        &self.private_key
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
    pub fn get_public_key_point(&self) -> CoreResult<neo_crypto::ECPoint> {
        neo_crypto::ECPoint::decode_compressed_with_curve(
            neo_crypto::ECCurve::secp256r1(),
            &self.compressed_public_key,
        )
        .map_err(|e| CoreError::InvalidOperation {
            message: format!("Failed to create ECPoint: {}", e),
        })
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
    pub fn sign(&self, data: &[u8]) -> CoreResult<Vec<u8>> {
        ECDsa::sign(data, &self.private_key, ECCurve::secp256r1())
            .map(|sig| sig.to_vec())
            .map_err(|e| CoreError::InvalidOperation {
                message: format!("Signing failed: {}", e),
            })
    }

    /// Verifies a signature against data.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> CoreResult<bool> {
        ECDsa::verify(data, signature, &self.public_key, ECCurve::secp256r1()).map_err(|e| {
            CoreError::InvalidOperation {
                message: format!("Verification failed: {}", e),
            }
        })
    }

    /// Exports the key pair to WIF format.
    pub fn to_wif(&self) -> String {
        Self::encode_wif(&self.private_key)
    }

    /// Exports the key pair to NEP-2 format.
    pub fn to_nep2(&self, password: &str, address_version: u8) -> CoreResult<String> {
        let encrypted = Self::encrypt_nep2(&self.private_key, password, address_version)?;
        // NEP-2 strings are Base58Check-encoded (C# KeyPair.Encrypt ->
        // Base58.Base58CheckEncode), yielding the standard "6P..." form. Base64
        // would produce a non-interoperable string that no other wallet accepts.
        Ok(base58::encode_check(&encrypted))
    }

    /// Decodes a WIF string to a private key.
    fn decode_wif(wif: &str) -> CoreResult<[u8; HASH_SIZE]> {
        let data = base58::decode_check(wif).map_err(map_wif_decode_error)?;

        if data.len() != 34 {
            return Err(CoreError::invalid_wif("invalid WIF length"));
        }

        // Check version byte
        if data[0] != 0x80 {
            return Err(CoreError::invalid_wif("invalid WIF version byte"));
        }

        // Check compressed flag
        if data[33] != 0x01 {
            return Err(CoreError::invalid_wif("invalid WIF compressed flag"));
        }

        let mut private_key = [0u8; HASH_SIZE];
        private_key.copy_from_slice(&data[1..33]);
        Ok(private_key)
    }

    /// Encodes a private key to WIF format.
    fn encode_wif(private_key: &[u8; HASH_SIZE]) -> String {
        let mut data = Vec::with_capacity(34);
        data.push(0x80); // Version byte for mainnet
        data.extend_from_slice(private_key);
        data.push(0x01); // Compressed flag

        base58::encode_check(&data)
    }

    /// Encrypts a private key using NEP-2 standard.
    fn encrypt_nep2(
        private_key: &[u8; HASH_SIZE],
        password: &str,
        address_version: u8,
    ) -> CoreResult<Vec<u8>> {
        // NEP-2 parameters
        let n = 16384; // CPU cost
        let r = 8; // Memory cost
        let p = 8; // Parallelization

        // Generate address hash
        let script_hash =
            UInt160::from_script(&Self::try_get_verification_script_for_key(private_key)?);
        let address = WalletAddress::to_address(&script_hash, address_version);
        let address_hash_full = neo_crypto::Crypto::hash256(address.as_bytes());
        let mut address_hash = [0u8; 4];
        address_hash.copy_from_slice(&address_hash_full[0..4]);

        // Derive key using scrypt
        let n: u32 = n;
        let params = Params::new(n.trailing_zeros() as u8, r, p, 64)
            .map_err(|e| CoreError::scrypt(e.to_string()))?;

        // Use Zeroizing wrapper to ensure sensitive data is cleared on drop
        let mut derived_key = Zeroizing::new([0u8; 64]);
        scrypt::scrypt(
            password.as_bytes(),
            &address_hash,
            &params,
            derived_key.as_mut(),
        )
        .map_err(|e| CoreError::scrypt(e.to_string()))?;

        // Split derived key
        let derived_half1 = &derived_key[0..HASH_SIZE];
        let derived_half2 = &derived_key[32..64];

        // XOR private key with derived_half1 (use Zeroizing for sensitive intermediate)
        let mut xor_key = Zeroizing::new([0u8; HASH_SIZE]);
        for i in 0..HASH_SIZE {
            xor_key[i] = private_key[i] ^ derived_half1[i];
        }

        // NEP-2 uses AES-256 in ECB mode with no padding over the 32-byte
        // XOR(privkey, derivedhalf1) (two independent 16-byte blocks), matching
        // C# KeyPair.Encrypt (CipherMode.ECB, PaddingMode.None). A CBC mode would
        // chain the second block and produce a non-interoperable encrypted key.
        let cipher =
            Aes256::new_from_slice(derived_half2).map_err(|e| CoreError::aes(e.to_string()))?;
        let mut block0 = GenericArray::clone_from_slice(&xor_key[0..16]);
        let mut block1 = GenericArray::clone_from_slice(&xor_key[16..32]);
        cipher.encrypt_block(&mut block0);
        cipher.encrypt_block(&mut block1);
        let mut encrypted = Vec::with_capacity(HASH_SIZE);
        encrypted.extend_from_slice(&block0);
        encrypted.extend_from_slice(&block1);

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
    ) -> CoreResult<[u8; HASH_SIZE]> {
        if encrypted_key.len() != 39 {
            return Err(CoreError::invalid_nep2_key("invalid NEP-2 key length"));
        }

        if &encrypted_key[0..2] != b"\x01\x42" {
            return Err(CoreError::invalid_nep2_key("invalid NEP-2 key prefix"));
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
        let params = Params::new(n.trailing_zeros() as u8, r, p, 64)
            .map_err(|e| CoreError::scrypt(e.to_string()))?;

        let mut derived_key = Zeroizing::new([0u8; 64]);
        scrypt::scrypt(
            password.as_bytes(),
            address_hash,
            &params,
            derived_key.as_mut(),
        )
        .map_err(|e| CoreError::scrypt(e.to_string()))?;

        let derived_half1 = &derived_key[0..HASH_SIZE];
        let derived_half2 = &derived_key[32..64];

        // AES-256-ECB no-padding over the two 16-byte blocks (C# parity).
        let cipher =
            Aes256::new_from_slice(derived_half2).map_err(|e| CoreError::aes(e.to_string()))?;
        let mut block0 = GenericArray::clone_from_slice(&encrypted_data[0..16]);
        let mut block1 = GenericArray::clone_from_slice(&encrypted_data[16..32]);
        cipher.decrypt_block(&mut block0);
        cipher.decrypt_block(&mut block1);
        let mut decrypted = Zeroizing::new([0u8; HASH_SIZE]);
        decrypted[0..16].copy_from_slice(&block0);
        decrypted[16..32].copy_from_slice(&block1);

        // XOR with derived_half1 to get private key
        let mut private_key = [0u8; HASH_SIZE];
        for i in 0..HASH_SIZE {
            private_key[i] = decrypted[i] ^ derived_half1[i];
        }

        // Verify by checking address hash
        let verification_script = Self::try_get_verification_script_for_key(&private_key)?;
        let script_hash = UInt160::from_script(&verification_script);
        let address = WalletAddress::to_address(&script_hash, address_version);
        let computed_hash_full = neo_crypto::Crypto::hash256(address.as_bytes());
        let computed_hash = &computed_hash_full[0..4];

        // Constant-time comparison to prevent timing oracle on password verification
        if !bool::from(computed_hash.ct_eq(address_hash)) {
            // Zeroize private key before returning error
            private_key.zeroize();
            return Err(CoreError::invalid_password(
                "invalid password for NEP-2 key",
            ));
        }

        Ok(private_key)
    }

    /// Gets verification script for a private key (helper function).
    /// Returns Result instead of panicking on failure.
    fn try_get_verification_script_for_key(private_key: &[u8; HASH_SIZE]) -> CoreResult<Vec<u8>> {
        let public_point =
            ECC::generate_public_key(private_key, ECCurve::secp256r1()).map_err(|e| {
                CoreError::InvalidOperation {
                    message: format!("Failed to generate public key: {}", e),
                }
            })?;
        let compressed =
            ECC::compress_public_key(&public_point).map_err(|e| CoreError::InvalidOperation {
                message: format!("Failed to compress public key: {}", e),
            })?;

        // Canonical CheckSig verification script (PUSHDATA1 + 33-byte pubkey +
        // SYSCALL + 4-byte LE hash of "System.Crypto.CheckSig"), matching C#
        // Contract.CreateSignatureRedeemScript. The previous hand-rolled script
        // used the wrong interop ("System.Crypto.CheckWitness") embedded as raw
        // ASCII, so the NEP-2 scrypt salt (address) diverged and '6P…' keys were
        // not interoperable with standard wallets.
        Ok(Helper::signature_redeem_script(&compressed))
    }
}

fn map_wif_decode_error(error: CryptoError) -> CoreError {
    match error {
        CryptoError::EncodingError { message } if message.starts_with("Base58 decode error: ") => {
            CoreError::Base58Decode {
                message: message
                    .trim_start_matches("Base58 decode error: ")
                    .to_string(),
            }
        }
        _ => CoreError::invalid_wif("invalid WIF format"),
    }
}

neo_primitives::impl_display_hex!(KeyPair, compressed_public_key);

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
#[path = "../tests/crypto/key_pair.rs"]
mod tests;

//! Hash function implementations for Neo blockchain.
//!
//! Provides SHA-256, SHA-512, SHA3-256/512, RIPEMD-160, Keccak-256, Blake2b/s hash functions.
//!
//! # Security
//! - Hash comparisons should use `ct_eq()` or `subtle::ConstantTimeEq` to prevent timing attacks
//!   when comparing hash values in security-sensitive contexts.

use crate::error::{CryptoError, CryptoResult};
use blake2::{Blake2b, Blake2b512, Blake2s256, digest::consts::U32};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256, Sha512};
use sha3::{Keccak256, Sha3_256, Sha3_512};
use subtle::ConstantTimeEq;

/// Hash algorithms supported by Neo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    /// SHA-256 (32 bytes output)
    Sha256,
    /// SHA-512 (64 bytes output)
    Sha512,
    /// Keccak-256 (32 bytes output, Ethereum compatible)
    Keccak256,
    /// RIPEMD-160 (20 bytes output)
    Ripemd160,
    /// Blake2b (64 bytes output)
    Blake2b,
    /// Blake2s (32 bytes output)
    Blake2s,
}

impl HashAlgorithm {
    /// Returns the C# `Neo.Cryptography.HashAlgorithm` byte, if this algorithm is
    /// part of that protocol enum.
    #[must_use]
    pub const fn to_neo_byte(self) -> Option<u8> {
        match self {
            Self::Sha256 => Some(0x00),
            Self::Keccak256 => Some(0x01),
            Self::Sha512 => Some(0x02),
            Self::Ripemd160 | Self::Blake2b | Self::Blake2s => None,
        }
    }

    /// Decodes a C# `Neo.Cryptography.HashAlgorithm` byte.
    #[must_use]
    pub const fn from_neo_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Sha256),
            0x01 => Some(Self::Keccak256),
            0x02 => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// Cryptographic hash functions for Neo blockchain.
///
/// This struct provides static methods for all hash functions used in Neo.
pub struct Crypto;

/// Incremental SHA-256 hasher for streaming inputs.
///
/// This is a thin newtype wrapper around `sha2::Sha256` that keeps direct
/// `sha2` usage contained to `neo-crypto` while exposing a small
/// `new`/`update`/`finalize` surface to higher-level crates that hash
/// async or chunked data without buffering it first.
#[derive(Clone, Default)]
pub struct Sha256Hasher(sha2::Sha256);

impl Sha256Hasher {
    /// Creates a new empty SHA-256 hasher.
    #[must_use]
    pub fn new() -> Self {
        Self(sha2::Sha256::new())
    }

    /// Adds bytes to the current hash state.
    pub fn update(&mut self, data: &[u8]) {
        sha2::Digest::update(&mut self.0, data);
    }

    /// Finalizes the hash and returns the 32-byte digest.
    #[must_use]
    pub fn finalize(self) -> [u8; 32] {
        sha2::Digest::finalize(self.0).into()
    }
}

fn blake2b_with_salt(data: &[u8], salt: &[u8], output_size: usize) -> CryptoResult<Vec<u8>> {
    if output_size == 0 || output_size > 64 {
        return Err(CryptoError::invalid_argument(
            "BLAKE2b output size must be between 1 and 64 bytes".to_string(),
        ));
    }

    Ok(blake2b_simd::Params::new()
        .hash_length(output_size)
        .salt(salt)
        .hash(data)
        .as_bytes()
        .to_vec())
}

impl Crypto {
    /// Computes SHA-256 hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 32-byte SHA-256 hash
    ///
    /// # Example
    /// ```
    /// use neo_crypto::Crypto;
    /// let hash = Crypto::sha256(b"Hello, Neo!");
    /// assert_eq!(hash.len(), 32);
    /// ```
    #[must_use]
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes SHA-512 hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 64-byte SHA-512 hash
    #[must_use]
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes Keccak-256 hash of the input data.
    ///
    /// This is the hash function used by Ethereum, provided for compatibility.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 32-byte Keccak-256 hash
    #[must_use]
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes SHA3-256 hash of the input data.
    #[must_use]
    pub fn sha3_256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes SHA3-512 hash of the input data.
    #[must_use]
    pub fn sha3_512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha3_512::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes RIPEMD-160 hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 20-byte RIPEMD-160 hash
    #[must_use]
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        let mut hasher = Ripemd160::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes BLAKE2b-512 hash of the input data (no salt).
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 64-byte `BLAKE2b` hash
    #[must_use]
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        Self::blake2b_512(data, None).expect("blake2b_512 without salt cannot fail")
    }

    /// Computes BLAKE2b-512 hash of the input data with an optional 16-byte salt.
    pub fn blake2b_512(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 64]> {
        let salt = salt.unwrap_or(&[]);
        if !salt.is_empty() && salt.len() != 16 {
            return Err(CryptoError::invalid_argument(
                "BLAKE2b salt must be 16 bytes or empty".to_string(),
            ));
        }

        if salt.is_empty() {
            let mut hasher = Blake2b512::new();
            Digest::update(&mut hasher, data);
            return Ok(hasher.finalize().into());
        }

        let result = blake2b_with_salt(data, salt, 64)?;
        let mut out = [0u8; 64];
        out.copy_from_slice(&result);
        Ok(out)
    }

    /// Computes BLAKE2b-256 hash of the input data with an optional 16-byte salt.
    pub fn blake2b_256(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 32]> {
        let salt = salt.unwrap_or(&[]);
        if !salt.is_empty() && salt.len() != 16 {
            return Err(CryptoError::invalid_argument(
                "BLAKE2b salt must be 16 bytes or empty".to_string(),
            ));
        }

        if salt.is_empty() {
            let mut hasher = Blake2b::<U32>::new();
            Digest::update(&mut hasher, data);
            let result = hasher.finalize();
            let mut out = [0u8; 32];
            out.copy_from_slice(&result);
            return Ok(out);
        }

        let result = blake2b_with_salt(data, salt, 32)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        Ok(out)
    }

    /// Computes BLAKE2s hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 32-byte BLAKE2s hash
    #[must_use]
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        let mut hasher = Blake2s256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes Hash160 (RIPEMD-160 of SHA-256).
    ///
    /// This is commonly used for Neo script hashes and addresses.
    /// Hash160(data) = RIPEMD160(SHA256(data))
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 20-byte Hash160 result
    ///
    /// # Example
    /// ```
    /// use neo_crypto::Crypto;
    /// let script_hash = Crypto::hash160(b"contract script");
    /// assert_eq!(script_hash.len(), 20);
    /// ```
    #[must_use]
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        let sha256_hash = Self::sha256(data);
        Self::ripemd160(&sha256_hash)
    }

    /// Computes Hash256 (double SHA-256).
    ///
    /// This is commonly used for Neo transaction and block hashes.
    /// Hash256(data) = SHA256(SHA256(data))
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 32-byte Hash256 result
    ///
    /// # Example
    /// ```
    /// use neo_crypto::Crypto;
    /// let tx_hash = Crypto::hash256(b"transaction data");
    /// assert_eq!(tx_hash.len(), 32);
    /// ```
    #[must_use]
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        let first_hash = Self::sha256(data);
        Self::sha256(&first_hash)
    }

    /// Computes hash using the specified algorithm.
    ///
    /// # Arguments
    /// * `algorithm` - Hash algorithm to use
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// Hash result as a `Vec<u8>` (length depends on algorithm)
    #[must_use]
    pub fn hash(algorithm: HashAlgorithm, data: &[u8]) -> Vec<u8> {
        match algorithm {
            HashAlgorithm::Sha256 => Self::sha256(data).to_vec(),
            HashAlgorithm::Sha512 => Self::sha512(data).to_vec(),
            HashAlgorithm::Keccak256 => Self::keccak256(data).to_vec(),
            HashAlgorithm::Ripemd160 => Self::ripemd160(data).to_vec(),
            HashAlgorithm::Blake2b => Self::blake2b(data).to_vec(),
            HashAlgorithm::Blake2s => Self::blake2s(data).to_vec(),
        }
    }
}

/// Constant-time hash comparison helpers grouped as associated functions.
pub struct CtCompare;

impl CtCompare {
    /// Compares two hash values in constant time to prevent timing attacks.
    ///
    /// This function is suitable for comparing hash values in security-sensitive
    /// contexts where timing side-channels could leak information.
    ///
    /// # Arguments
    /// * `a` - First hash value
    /// * `b` - Second hash value
    ///
    /// # Returns
    /// `true` if the hashes are equal, `false` otherwise. The comparison takes
    /// the same amount of time regardless of where the hashes differ.
    ///
    /// # Example
    /// ```
    /// use neo_crypto::Crypto;
    /// use neo_crypto::hash::CtCompare;
    ///
    /// let hash1 = Crypto::sha256(b"message");
    /// let hash2 = Crypto::sha256(b"message");
    /// let hash3 = Crypto::sha256(b"different");
    ///
    /// assert!(CtCompare::ct_hash_eq(&hash1, &hash2));
    /// assert!(!CtCompare::ct_hash_eq(&hash1, &hash3));
    /// ```
    #[must_use]
    pub fn ct_hash_eq<const N: usize>(a: &[u8; N], b: &[u8; N]) -> bool {
        a.ct_eq(b).into()
    }

    /// Compares two hash byte slices in constant time.
    ///
    /// Returns `false` immediately if the slices have different lengths.
    /// Otherwise, performs a constant-time comparison of the contents.
    ///
    /// # Arguments
    /// * `a` - First hash slice
    /// * `b` - Second hash slice
    ///
    /// # Returns
    /// `true` if the slices have the same length and content, `false` otherwise.
    #[must_use]
    pub fn ct_hash_slice_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }
}

#[cfg(test)]
mod tests;

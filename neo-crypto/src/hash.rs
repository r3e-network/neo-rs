//! Hash function implementations for Neo blockchain.
//!
//! Provides SHA-256, SHA-512, RIPEMD-160, Keccak-256, Blake2b/s hash functions.

use blake2::{Blake2b512, Blake2s256};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256, Sha512};
use sha3::Keccak256;

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

/// Cryptographic hash functions for Neo blockchain.
///
/// This struct provides static methods for all hash functions used in Neo.
pub struct Crypto;

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
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes SHA-512 hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 64-byte SHA-512 hash
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        hasher.update(data);
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
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes RIPEMD-160 hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 20-byte RIPEMD-160 hash
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        let mut hasher = Ripemd160::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes BLAKE2b hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 64-byte BLAKE2b hash
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        let mut hasher = Blake2b512::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Computes BLAKE2s hash of the input data.
    ///
    /// # Arguments
    /// * `data` - Input bytes to hash
    ///
    /// # Returns
    /// 32-byte BLAKE2s hash
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        let mut hasher = Blake2s256::new();
        hasher.update(data);
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
    /// Hash result as a Vec<u8> (length depends on algorithm)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let hash = Crypto::sha256(b"hello");
        assert_eq!(hash.len(), 32);
        // Known SHA-256 hash of "hello"
        let expected = hex::decode("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824").unwrap();
        assert_eq!(hash.to_vec(), expected);
    }

    #[test]
    fn test_sha512() {
        let hash = Crypto::sha512(b"hello");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_ripemd160() {
        let hash = Crypto::ripemd160(b"hello");
        assert_eq!(hash.len(), 20);
    }

    #[test]
    fn test_hash160() {
        let hash = Crypto::hash160(b"hello");
        assert_eq!(hash.len(), 20);
        // Hash160 should be RIPEMD160(SHA256(data))
        let sha256 = Crypto::sha256(b"hello");
        let expected = Crypto::ripemd160(&sha256);
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_hash256() {
        let hash = Crypto::hash256(b"hello");
        assert_eq!(hash.len(), 32);
        // Hash256 should be SHA256(SHA256(data))
        let first = Crypto::sha256(b"hello");
        let expected = Crypto::sha256(&first);
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_keccak256() {
        let hash = Crypto::keccak256(b"hello");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_blake2b() {
        let hash = Crypto::blake2b(b"hello");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_blake2s() {
        let hash = Crypto::blake2s(b"hello");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_hash_algorithm() {
        let data = b"test data";

        assert_eq!(Crypto::hash(HashAlgorithm::Sha256, data).len(), 32);
        assert_eq!(Crypto::hash(HashAlgorithm::Sha512, data).len(), 64);
        assert_eq!(Crypto::hash(HashAlgorithm::Ripemd160, data).len(), 20);
        assert_eq!(Crypto::hash(HashAlgorithm::Keccak256, data).len(), 32);
        assert_eq!(Crypto::hash(HashAlgorithm::Blake2b, data).len(), 64);
        assert_eq!(Crypto::hash(HashAlgorithm::Blake2s, data).len(), 32);
    }
}

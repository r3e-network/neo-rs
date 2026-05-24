//! Neo-specific hash facade.

use crate::hash::Crypto;
use crate::{murmur, CryptoResult};

/// Neo-specific hash functions.
///
/// This is a convenience wrapper around [`Crypto`] that provides the same
/// hash functions. For new code, prefer using [`Crypto`] directly.
pub struct NeoHash;

impl NeoHash {
    /// Computes SHA-256 hash of the input data.
    #[inline]
    #[must_use]
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        Crypto::sha256(data)
    }

    /// Computes SHA-512 hash of the input data.
    #[inline]
    #[must_use]
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        Crypto::sha512(data)
    }

    /// Computes Keccak-256 hash of the input data.
    #[inline]
    #[must_use]
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        Crypto::keccak256(data)
    }

    /// Computes SHA3-256 hash of the input data.
    #[inline]
    #[must_use]
    pub fn sha3_256(data: &[u8]) -> [u8; 32] {
        Crypto::sha3_256(data)
    }

    /// Computes SHA3-512 hash of the input data.
    #[inline]
    #[must_use]
    pub fn sha3_512(data: &[u8]) -> [u8; 64] {
        Crypto::sha3_512(data)
    }

    /// Computes RIPEMD-160 hash of the input data.
    #[inline]
    #[must_use]
    pub fn ripemd160(data: &[u8]) -> [u8; 20] {
        Crypto::ripemd160(data)
    }

    /// Computes `BLAKE2b` hash of the input data.
    #[inline]
    #[must_use]
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        Crypto::blake2b(data)
    }

    /// Computes BLAKE2b-512 hash of the input data with optional salt.
    #[inline]
    pub fn blake2b_512(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 64]> {
        Crypto::blake2b_512(data, salt)
    }

    /// Computes BLAKE2b-256 hash of the input data with optional salt.
    #[inline]
    pub fn blake2b_256(data: &[u8], salt: Option<&[u8]>) -> CryptoResult<[u8; 32]> {
        Crypto::blake2b_256(data, salt)
    }

    /// Computes BLAKE2s hash of the input data.
    #[inline]
    #[must_use]
    pub fn blake2s(data: &[u8]) -> [u8; 32] {
        Crypto::blake2s(data)
    }

    /// Computes Hash160 (RIPEMD-160 of SHA-256).
    #[inline]
    #[must_use]
    pub fn hash160(data: &[u8]) -> [u8; 20] {
        Crypto::hash160(data)
    }

    /// Computes Hash256 (double SHA-256).
    #[inline]
    #[must_use]
    pub fn hash256(data: &[u8]) -> [u8; 32] {
        Crypto::hash256(data)
    }

    /// Computes Murmur128 hash (x64 variant) used by Neo runtime.
    #[must_use]
    pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
        murmur::murmur128(data, seed)
    }
}

#[cfg(test)]
mod tests {
    use super::NeoHash;

    #[test]
    fn test_hash_functions() {
        let data = b"hello world";

        let sha256_hash = NeoHash::sha256(data);
        assert_eq!(sha256_hash.len(), 32);

        let hash160 = NeoHash::hash160(data);
        assert_eq!(hash160.len(), 20);

        let hash256 = NeoHash::hash256(data);
        assert_eq!(hash256.len(), 32);
    }
}

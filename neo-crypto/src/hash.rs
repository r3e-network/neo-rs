//! Hash function implementations for Neo blockchain.
//!
//! Provides SHA-256, SHA-512, SHA3-256/512, RIPEMD-160, Keccak-256, Blake2b/s hash functions.

use crate::error::{CryptoError, CryptoResult};
use blake2::{
    digest::{
        block_buffer::BlockBuffer,
        consts::U32,
        core_api::{BlockSizeUser, BufferKindUser, UpdateCore, VariableOutputCore},
        Output,
    },
    Blake2b, Blake2b512, Blake2bVarCore, Blake2s256,
};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256, Sha512};
use sha3::{Keccak256, Sha3_256, Sha3_512};

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

fn blake2b_with_salt(data: &[u8], salt: &[u8], output_size: usize) -> CryptoResult<Vec<u8>> {
    if output_size == 0 || output_size > 64 {
        return Err(CryptoError::invalid_argument(
            "BLAKE2b output size must be between 1 and 64 bytes".to_string(),
        ));
    }

    let mut core = Blake2bVarCore::new_with_params(salt, &[], 0, output_size);
    let mut buffer = BlockBuffer::<
        <Blake2bVarCore as BlockSizeUser>::BlockSize,
        <Blake2bVarCore as BufferKindUser>::BufferKind,
    >::default();
    buffer.digest_blocks(data, |blocks| core.update_blocks(blocks));

    let mut full = Output::<Blake2bVarCore>::default();
    core.finalize_variable_core(&mut buffer, &mut full);
    Ok(full[..output_size].to_vec())
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
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes SHA3-256 hash of the input data.
    pub fn sha3_256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        Digest::update(&mut hasher, data);
        hasher.finalize().into()
    }

    /// Computes SHA3-512 hash of the input data.
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
    /// 64-byte BLAKE2b hash
    pub fn blake2b(data: &[u8]) -> [u8; 64] {
        Self::blake2b_512(data, None).expect("blake2b_512 without salt cannot fail")
    }

    /// Computes BLAKE2b-512 hash of the input data with an optional 16-byte salt.
    pub fn blake2b_512(
        data: &[u8],
        salt: Option<&[u8]>,
    ) -> CryptoResult<[u8; 64]> {
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
    pub fn blake2b_256(
        data: &[u8],
        salt: Option<&[u8]>,
    ) -> CryptoResult<[u8; 32]> {
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
        let expected =
            hex::decode("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
                .unwrap();
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
    fn test_sha3_256() {
        let hash = Crypto::sha3_256(b"hello world");
        let expected = hex::decode("644bcc7e564373040999aac89e7622f3ca71fba1d972fd94a31c3bfbf24e3938")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);
    }

    #[test]
    fn test_sha3_512() {
        let hash = Crypto::sha3_512(b"hello world");
        let expected = hex::decode("840006653e9ac9e95117a15c915caab81662918e925de9e004f774ff82d7079a40d4d27b1b372657c61d46d470304c88c788b3a4527ad074d1dccbee5dbaa99a")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);
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
    fn test_blake2b_512() {
        let hash = Crypto::blake2b_512(b"hello world", None).unwrap();
        let expected = hex::decode("021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);

        let salt = b"0123456789abcdef";
        let hash = Crypto::blake2b_512(b"hello world", Some(salt)).unwrap();
        let expected = hex::decode("d986f099932b14a65ebc5a6fb1b8bff8d05b6924a4ff74d4972949b880c1f74b5ab263357f332726d98fac3cabeacf415099f1a2a9b97b66cd989ca865539640")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);

        assert!(Crypto::blake2b_512(b"abc", Some(&[0u8; 15])).is_err());
        assert!(Crypto::blake2b_512(b"abc", Some(&[0u8; 17])).is_err());
    }

    #[test]
    fn test_blake2b_256() {
        let hash = Crypto::blake2b_256(b"hello world", None).unwrap();
        let expected = hex::decode("256c83b297114d201b30179f3f0ef0cace9783622da5974326b436178aeef610")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);

        let salt = b"0123456789abcdef";
        let hash = Crypto::blake2b_256(b"hello world", Some(salt)).unwrap();
        let expected = hex::decode("779c5f2194a9c2c03e73e3ffcf3e1508dd83cb85cd861029415ab961a755cc4e")
            .unwrap();
        assert_eq!(hash.to_vec(), expected);

        assert!(Crypto::blake2b_256(b"abc", Some(&[0u8; 15])).is_err());
        assert!(Crypto::blake2b_256(b"abc", Some(&[0u8; 17])).is_err());
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

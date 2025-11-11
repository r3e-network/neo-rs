use sha2::{Digest as ShaDigest, Sha512};

use crate::hash::types::{Hash160, Hash256};

/// Compute a single round SHA-256 hash over the provided bytes.
#[inline]
pub fn sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(data.as_ref());
    h.finalize().into()
}

/// Compute two rounds of SHA-256 – the default block hashing strategy in Neo.
#[inline]
pub fn double_sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    sha256(sha256(data))
}

/// Compute RIPEMD-160 hash.
#[inline]
pub fn ripemd160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let mut ripemd = ripemd::Ripemd160::new();
    ripemd.update(data.as_ref());
    ripemd.finalize().into()
}

/// Compute RIPEMD-160(SHA-256(data)) which is used for script hashes.
#[inline]
pub fn hash160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let sha = sha256(data);
    ripemd160(sha)
}

/// Produce a typed `Hash160` from the provided payload.
#[inline]
pub fn hash160_typed<T: AsRef<[u8]>>(data: T) -> Hash160 {
    Hash160::from_slice(&hash160(data)).expect("hash160 output is 20 bytes")
}

/// Compute Keccak-256 hash.
#[inline]
pub fn keccak256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    use sha3::Digest as _;
    let mut hasher = sha3::Keccak256::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

/// Compute a single round SHA-512 hash.
#[inline]
pub fn sha512<T: AsRef<[u8]>>(data: T) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

/// Produce a typed `Hash256` from the provided payload.
#[inline]
pub fn double_sha256_typed<T: AsRef<[u8]>>(data: T) -> Hash256 {
    Hash256::from_slice(&double_sha256(data)).expect("double sha256 output is 32 bytes")
}

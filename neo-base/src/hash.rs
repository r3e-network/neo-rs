// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use sha2::Digest;

use crate::bytes::ToArray;

pub const SHA256_HASH_SIZE: usize = 32;
pub const RIPEMD160_HASH_SIZE: usize = 20;
pub const KECCAK256_HASH_SIZE: usize = 32;

pub trait Sha256 {
    fn sha256(&self) -> [u8; SHA256_HASH_SIZE];
}

impl<T: AsRef<[u8]>> Sha256 for T {
    #[inline]
    fn sha256(&self) -> [u8; SHA256_HASH_SIZE] {
        let mut h = sha2::Sha256::new();
        h.update(self);

        h.finalize().as_slice().to_array()
    }
}

pub trait SlicesSha256 {
    fn slices_sha256(self) -> [u8; SHA256_HASH_SIZE];
}

impl<T: Iterator> SlicesSha256 for T
where
    <T as Iterator>::Item: AsRef<[u8]>,
{
    #[inline]
    fn slices_sha256(self) -> [u8; SHA256_HASH_SIZE] {
        let mut h = sha2::Sha256::new();
        self.for_each(|s| h.update(s));

        h.finalize().as_slice().to_array()
    }
}

pub trait Sha256Twice {
    fn sha256_twice(&self) -> [u8; SHA256_HASH_SIZE];
}

impl<T: Sha256> Sha256Twice for T {
    #[inline]
    fn sha256_twice(&self) -> [u8; SHA256_HASH_SIZE] { self.sha256().sha256() }
}

pub trait Ripemd160 {
    fn ripemd160(&self) -> [u8; RIPEMD160_HASH_SIZE];
}

impl<T: AsRef<[u8]>> Ripemd160 for T {
    #[inline]
    fn ripemd160(&self) -> [u8; RIPEMD160_HASH_SIZE] {
        let mut h = ripemd::Ripemd160::new();
        h.update(self);

        h.finalize().as_slice().to_array()
    }
}

pub trait NetSha256 {
    fn net_sha256(&self, network: u32) -> [u8; SHA256_HASH_SIZE];
}

impl<T: Sha256> NetSha256 for T {
    #[inline]
    fn net_sha256(&self, network: u32) -> [u8; SHA256_HASH_SIZE] {
        [network.to_le_bytes().as_slice(), &self.sha256().as_slice()].iter().slices_sha256()
    }
}

pub trait Sha256Checksum {
    fn sha256_checksum(&self) -> u32;
}

impl<T: Sha256> Sha256Checksum for T {
    #[inline]
    fn sha256_checksum(&self) -> u32 {
        let h = self.sha256().sha256();
        u32::from_le_bytes([h[0], h[1], h[2], h[3]])
    }
}

pub trait Keccak256 {
    fn keccak256(&self) -> [u8; KECCAK256_HASH_SIZE];
}

impl<T: AsRef<[u8]>> Keccak256 for T {
    fn keccak256(&self) -> [u8; KECCAK256_HASH_SIZE] {
        use tiny_keccak::{Hasher, Keccak};

        let mut keccak = Keccak::v256();
        let mut hash = [0_u8; KECCAK256_HASH_SIZE];

        keccak.update(self.as_ref());
        keccak.finalize(&mut hash);

        hash
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encoding::hex::ToHex;

    #[test]
    fn test_ripemd160() {
        let hash = b"Hello world!".ripemd160();
        assert_eq!(hash.to_hex(), "7f772647d88750add82d8e1a7a3e5c0902a346a3");

        let hash = b"Hello world!".sha256().ripemd160();
        assert_eq!(hash.to_hex(), "621281c15fb62d5c6013ea29007491e8b174e1b9");
    }

    #[test]
    fn test_sha256() {
        let hash = b"Hello world!".sha256();
        assert_eq!(
            hash.to_hex(),
            "c0535e4be2b79ffd93291305436bf889314e4a3faec05ecffcbb7df31ad9e51a"
        );

        let hash = b"hello world".sha256();
        assert_eq!(
            hash.to_hex(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        let hash = b"".sha256();
        assert_eq!(
            hash.to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_slices() {
        let hash = [b"Hello world!".as_ref(), b"".as_ref()].iter().slices_sha256();
        assert_eq!(
            hash.to_hex(),
            "c0535e4be2b79ffd93291305436bf889314e4a3faec05ecffcbb7df31ad9e51a"
        );
    }
}

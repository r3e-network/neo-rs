extern crate hex;
extern crate sha2;
extern crate ripemd160;
extern crate assert;

use sha2::{Sha256, Digest};
use ripemd160::Ripemd160;
use hex::encode;
use assert::{assert_eq, assert_ne};

fn sha256(input: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().to_vec()
}

fn double_sha256(input: &[u8]) -> Vec<u8> {
    let first_sha = sha256(input);
    sha256(&first_sha)
}

fn ripemd160(input: &[u8]) -> Vec<u8> {
    let mut hasher = Ripemd160::new();
    hasher.update(input);
    hasher.finalize().to_vec()
}

fn hash160(input: &[u8]) -> Vec<u8> {
    let sha = sha256(input);
    ripemd160(&sha)
}

fn checksum(data: &[u8]) -> [u8; 4] {
    let hash = double_sha256(data);
    let mut checksum = [0u8; 4];
    checksum.copy_from_slice(&hash[..4]);
    checksum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn test_sha256() {
        let input = b"hello";
        let data = sha256(input);

        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let actual = encode(data);

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_double_sha256() {
        let input = b"hello";
        let data = double_sha256(input);

        let first_sha = sha256(input);
        let double_sha = sha256(&first_sha);
        let expected = encode(double_sha);

        let actual = encode(data);
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_ripemd160() {
        let input = b"hello";
        let data = ripemd160(input);

        let expected = "108f07b8382412612c048d07d13f814118445acd";
        let actual = encode(data);
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_hash160() {
        let input = "02cccafb41b220cab63fd77108d2d1ebcffa32be26da29a04dca4996afce5f75db";
        let public_key_bytes = hex::decode(input).unwrap();
        let data = hash160(&public_key_bytes);

        let expected = "c8e2b685cc70ec96743b55beb9449782f8f775d8";
        let actual = encode(data);
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_checksum() {
        let test_cases = vec![
            (vec![], 0xe2e0f65d),
            (vec![1, 2, 3, 4], 0xe272e48d),
        ];

        for (data, sum) in test_cases {
            let checksum_bytes = checksum(&data);
            let checksum_u32 = u32::from_le_bytes(checksum_bytes);
            assert_eq!(sum, checksum_u32);
        }
    }
}

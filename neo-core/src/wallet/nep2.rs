// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};

use bytes::{BufMut, BytesMut};
use subtle::ConstantTimeEq;

use neo_base::bytes::{xor_array, ToArray, ToRevArray};
use neo_base::encoding::base58::{FromBase58Check, ToBase58Check};
use neo_base::errors;
use neo_base::hash::Sha256Twice;
use neo_crypto::aes::Aes256EcbCipher;
use neo_crypto::key::SecretKey;
use neo_crypto::scrypt::{DeriveKey, Params};

use crate::types::{Address, ToNeo3Address};
use crate::{PrivateKey, PublicKey, KEY_SIZE};

const NEP2_KEY_SIZE: usize = 39;
const DERIVED_KEY_SIZE: usize = 2 * KEY_SIZE;

#[derive(Debug)]
pub struct Nep2Key {
    key: String,
}

impl Nep2Key {
    #[inline]
    pub fn as_str(&self) -> &str {
        self.key.as_str()
    }
}

pub trait ToNep2Key {
    fn to_nep2_key(&self, addr: &Address, password: &[u8]) -> Nep2Key;
}

pub const fn scrypt_params() -> Params {
    Params { n: 16384, p: 8, r: 8, len: 64 }
}

impl ToNep2Key for PrivateKey {
    /// NOTE: there is no normalization for password
    fn to_nep2_key(&self, addr: &Address, password: &[u8]) -> Nep2Key {
        let hash = addr.as_str().sha256_twice();

        let derived = password
            .derive_key::<64>(&hash[..4], scrypt_params())
            .expect("default nep2-key params should be ok");

        let derived: &[u8; 64] = derived.as_ref();
        let sk = SecretKey::<KEY_SIZE>::from(self.as_le_bytes().to_rev_array());
        let mut key = xor_array::<KEY_SIZE>(sk.as_bytes(), &derived[..KEY_SIZE]);

        let _ = SecretKey::from(derived[KEY_SIZE..].to_array())
            .aes256_ecb_encrypt_aligned(key.as_mut_slice())
            .expect("`aes256_ecb_encrypt_aligned` should be ok");

        let mut buf = BytesMut::with_capacity(3 + 4 + key.len());

        buf.put_u8(0x01);
        buf.put_u8(0x42);
        buf.put_u8(0xe0);
        buf.put_slice(&hash[..4]);
        buf.put_slice(&key);

        Nep2Key { key: buf.to_base58_check(None, None) }
    }
}

#[derive(Debug, Clone, errors::Error)]
pub enum Nep2VerifyError {
    #[error("nep2-key: invalid base58check")]
    InvalidBase58Check,

    #[error("nep2-key: the key length(base58-decoded) must be 39")]
    InvalidKeyLength,

    #[error("nep2-key invalid key hash")]
    InvalidHash,

    #[error("nep2-key: invalid nep2 key")]
    InvalidKey,
}

pub trait Nep2KeyDecrypt {
    fn decrypt_nep2_key(&self, nep2_key: &str) -> Result<PrivateKey, Nep2VerifyError>;
}

impl<T: AsRef<[u8]>> Nep2KeyDecrypt for T {
    /// self is the password,
    /// NOTE: there is no normalization for password
    fn decrypt_nep2_key(&self, nep2_key: &str) -> Result<PrivateKey, Nep2VerifyError> {
        let raw = Vec::from_base58_check(nep2_key, None, None)
            .map_err(|_| Nep2VerifyError::InvalidBase58Check)?;

        if raw.len() != NEP2_KEY_SIZE {
            return Err(Nep2VerifyError::InvalidKeyLength);
        }

        let derived = self
            .derive_key::<DERIVED_KEY_SIZE>(&raw[3..7], scrypt_params())
            .expect("default nep2-key params should be ok");

        let derived = derived.as_bytes();
        let mut encrypted: [u8; KEY_SIZE] = raw[7..].to_array();

        let _ = SecretKey::from(derived[KEY_SIZE..].to_array())
            .aes256_ecb_decrypt_aligned(encrypted.as_mut_slice())
            .expect("`aes256_ecb_decrypt_aligned` should be ok");

        // secret-key
        let sk = xor_array::<KEY_SIZE>(&encrypted, &derived[..KEY_SIZE]);

        let sk =
            PrivateKey::from_be_bytes(sk.as_slice()).map_err(|_err| Nep2VerifyError::InvalidKey)?;

        let addr =
            PublicKey::try_from(&sk).map_err(|_err| Nep2VerifyError::InvalidKey)?.to_neo3_address();

        let hash = addr.as_str().sha256_twice();
        if hash[..4].ct_eq(&raw[3..7]).into() {
            Ok(sk)
        } else {
            Err(Nep2VerifyError::InvalidHash)
        }
    }
}

#[cfg(test)]
mod test {
    use neo_base::encoding::hex::{DecodeHex, ToHex};

    use super::*;

    #[test]
    #[ignore = "It is too time-consuming"]
    fn test_nep2_key() {
        let sk = "7d128a6d096f0c14c3a25a2b0c41cf79661bfcb4a8cc95aaaea28bde4d732344"
            .decode_hex()
            .expect("hex decode should be ok");

        let sk = PrivateKey::from_be_bytes(sk.as_slice()).expect("from le-bytes should be ok ");

        let pk = PublicKey::try_from(&sk).expect("from private key should be ok");
        assert_eq!(
            "02028a99826edc0c97d18e22b6932373d908d323aa7f92656a77ec26e8861699ef",
            pk.to_compressed().to_hex(),
        );

        let addr = pk.to_neo3_address();
        assert_eq!(addr.as_str(), "NPTmAHDxo6Pkyic8Nvu3kwyXoYJCvcCB6i");

        let pwd = "city of zion";
        let key = sk.to_nep2_key(&addr, pwd.as_bytes());

        assert_eq!(key.as_str(), "6PYUUUFei9PBBfVkSn8q7hFCnewWFRBKPxcn6Kz6Bmk3FqWyLyuTQE2XFH");

        let got = pwd.decrypt_nep2_key(key.as_str()).expect("decrypt should be ok");
        assert_eq!(got.as_le_bytes(), sk.as_le_bytes());
    }
}

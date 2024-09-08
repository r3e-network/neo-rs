// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use bytes::BytesMut;

use neo_base::encoding::bin::HashFieldsSha256;
use neo_crypto::ecdsa::{DigestVerify, ECC256_SIGN_SIZE, Secp256r1Sign, Sign as EcdsaSign, SignError};

use crate::{PrivateKey, PublicKey};
use crate::types::FixedBytes;

pub type Sign = FixedBytes<ECC256_SIGN_SIZE>;

impl Sign {
    pub fn to_invocation_script(&self) -> Script {
        let mut buf = BytesMut::with_capacity(4 + ECC256_SIGN_SIZE);
        buf.put_varbytes(self.as_bytes());
        buf.into()
    }

    pub fn as_secp256r1_sign(&self) -> Secp256r1Sign {
        Secp256r1Sign::from(self.0.clone())
    }
}

impl Into<Secp256r1Sign> for Sign {
    fn into(self) -> Secp256r1Sign { Secp256r1Sign::from(self.0) }
}

impl From<Secp256r1Sign> for Sign {
    fn from(sign: Secp256r1Sign) -> Self { Self(sign.into()) }
}

pub struct SignData([u8; SIGN_DATA_SIZE]);

impl SignData {
    pub fn network(&self) -> u32 {
        u32::from_le_bytes([self.0[0], self.0[1], self.0[2], self.0[3]])
    }

    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl AsRef<[u8]> for SignData {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl AsRef<[u8; SIGN_DATA_SIZE]> for SignData {
    fn as_ref(&self) -> &[u8; SIGN_DATA_SIZE] { &self.0 }
}


pub trait ToSignData {
    fn to_sign_data(&self, network: u32) -> SignData;
}

impl<T: HashFieldsSha256> ToSignData for T {
    fn to_sign_data(&self, network: u32) -> SignData {
        let mut data = SignData([0u8; SIGN_DATA_SIZE]);
        let hash = self.hash_fields_sha256();

        data.0[..4].copy_from_slice(&network.to_le_bytes());
        data.0[4..].copy_from_slice(&hash);
        data
    }
}


pub trait ToSign {
    fn to_sign(&self, network: u32, key: &PrivateKey) -> Result<Sign, SignError>;
}

impl<T: ToSignData> ToSign for T {
    fn to_sign(&self, network: u32, key: &PrivateKey) -> Result<Sign, SignError> {
        let data = self.to_sign_data(network);
        key.sign(data.as_bytes())
            .map(|sign| Sign::from(sign))
    }
}


pub trait SignVerify {
    type Sign: ?Sized;

    fn verify_sign(&self, key: &PublicKey, sign: &Self::Sign, network: u32) -> bool;
}

impl<T: HashFieldsSha256> SignVerify for T {
    type Sign = [u8];

    fn verify_sign(&self, key: &PublicKey, sign: &Self::Sign, network: u32) -> bool {
        let sign_data = self.to_sign_data(network);
        key.verify_digest(&sign_data, sign).is_ok()
    }
}

pub trait MultiSignVerify {
    type Sign: ?Sized;

    fn verify_multi_sign(&self, keys: &[PublicKey], signs: &[&Self::Sign], network: u32) -> bool;
}

impl<T: HashFieldsSha256> MultiSignVerify for T {
    type Sign = [u8];

    fn verify_multi_sign(&self, keys: &[PublicKey], signs: &[&Self::Sign], network: u32) -> bool {
        if keys.is_empty() || signs.is_empty() || signs.len() > keys.len() {
            return false;
        }

        let mut s = 0usize;
        let sign_data = self.to_sign_data(network);
        for (k, key) in keys.iter().enumerate() {
            if keys.len() - k < signs.len() - s {
                return false;
            }

            if s < signs.len() && key.verify_digest(&sign_data, signs[s]).is_ok() {
                s += 1;
            }
        }

        true
    }
}


#[cfg(test)]
mod test {
    use neo_base::hash::{Sha256, SHA256_HASH_SIZE};

    use super::*;

    struct MockHashFields;

    impl HashFieldsSha256 for MockHashFields {
        fn hash_fields_sha256(&self) -> [u8; SHA256_HASH_SIZE] { "hello".sha256() }
    }

    #[test]
    fn test_sign_data() {
        let fields = MockHashFields;
        let sign = fields.to_sign_data(0x11223344);
        assert_eq!(sign.network(), 0x11223344);
        assert_eq!(&sign.as_bytes()[4..], "hello".sha256().as_slice());
    }
}
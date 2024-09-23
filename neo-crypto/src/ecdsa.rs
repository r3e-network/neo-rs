// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};

use p256::ecdsa::{
    signature::{RandomizedSigner, Verifier as P256Verifier},
    Signature, VerifyingKey, SigningKey,
};

use neo_base::bytes::{ToArray, ToRevArray};
use neo_base::errors;

use crate::secp256r1;

// const ECC256_SIZE: usize = 32;
pub const ECC256_SIGN_SIZE: usize = 32 * 2;

pub trait Sign {
    type Sign;
    type Error;

    fn sign<T: AsRef<[u8]>>(&self, message: T) -> Result<Self::Sign, Self::Error>;
}

pub trait Verify {
    type Sign;
    type Error;

    fn verify<T: AsRef<[u8]>>(&self, message: T, sign: &Self::Sign) -> Result<(), Self::Error>;
}

pub trait DigestVerify {
    type Error;

    fn verify_digest<T: AsRef<[u8]>, S: AsRef<[u8]>>(&self, message: T, sign: S) -> Result<(), Self::Error>;
}

pub struct Secp256r1Sign {
    sign: [u8; ECC256_SIGN_SIZE],
}

impl AsRef<[u8]> for Secp256r1Sign {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.sign.as_ref() }
}

impl AsRef<[u8; ECC256_SIGN_SIZE]> for Secp256r1Sign {
    #[inline]
    fn as_ref(&self) -> &[u8; ECC256_SIGN_SIZE] { &self.sign }
}

impl From<[u8; ECC256_SIGN_SIZE]> for Secp256r1Sign {
    #[inline]
    fn from(sign: [u8; ECC256_SIGN_SIZE]) -> Self { Secp256r1Sign { sign } }
}

impl Into<[u8; ECC256_SIGN_SIZE]> for Secp256r1Sign {
    #[inline]
    fn into(self) -> [u8; ECC256_SIGN_SIZE] { self.sign }
}

#[derive(Debug, Clone, errors::Error)]
pub enum SignError {
    #[error("ecdsa: invalid private key")]
    InvalidPrivateKey,

    #[error("ecdsa: sign error: {0}")]
    SignError(String),
}

#[cfg(not(feature = "sgx"))]
impl Sign for secp256r1::PrivateKey {
    type Sign = Secp256r1Sign;
    type Error = SignError;

    fn sign<T: AsRef<[u8]>>(&self, message: T) -> Result<Self::Sign, Self::Error> {
        use secp256r1::KEY_SIZE;

        let key: [u8; KEY_SIZE] = self.as_le_bytes().to_rev_array();
        let sk: SigningKey = p256::SecretKey::from_slice(&key)
            .map(|key| key.into())
            .map_err(|_| SignError::InvalidPrivateKey)?;

        let mut rnd = rand_core::OsRng;
        let sign: Signature = sk
            .try_sign_with_rng(&mut rnd, message.as_ref())
            .map_err(|err| SignError::SignError(err.to_string()))?;

        let sign = sign.to_bytes();
        // sign[0..32].reverse(); // use little endian
        // sign[32..64].reverse();
        Ok(Secp256r1Sign { sign: sign.to_array() })
    }
}

#[derive(Debug, Clone, errors::Error)]
pub enum VerifyError {
    #[error("ecdsa: invalid public key")]
    InvalidPublicKey,

    #[error("ecdsa: invalid signature")]
    InvalidSignature,
}

impl Verify for secp256r1::PublicKey {
    type Sign = Secp256r1Sign;
    type Error = VerifyError;

    #[inline]
    fn verify<T: AsRef<[u8]>>(&self, message: T, sign: &Self::Sign) -> Result<(), Self::Error> {
        self.verify_digest(message, sign)
    }
}

impl DigestVerify for secp256r1::PublicKey {
    type Error = VerifyError;

    fn verify_digest<T: AsRef<[u8]>, S: AsRef<[u8]>>(&self, message: T, sign: S) -> Result<(), Self::Error> {
        // let mut sign: [u8; ECC256_SIGN_SIZE] = sign.as_ref().clone().to_array();
        // sign[0..32].reverse();
        // sign[32..ECC256_SIGN_SIZE].reverse(); // little endian to big endian

        let sign = Signature::try_from(sign.as_ref())
            .map_err(|_| VerifyError::InvalidSignature)?;

        VerifyingKey::from_sec1_bytes(&self.to_uncompressed())
            .map_err(|_| VerifyError::InvalidPublicKey)?
            .verify(message.as_ref(), &sign)
            .map_err(|_| VerifyError::InvalidSignature)
    }
}

#[cfg(test)]
mod test {
    use neo_base::encoding::hex::DecodeHex;

    use crate::secp256r1::{PrivateKey, PublicKey};
    use super::*;

    #[test]
    fn test_p256_sign() {
        let data = b"hello world";
        let sk = "f72b8fab85fdcc1bdd20b107e5da1ab4713487bc88fc53b5b134f5eddeaa1a19"
            .decode_hex()
            .expect("hex decode ok");

        let sk = PrivateKey::from_be_bytes(sk.as_slice())
            .expect("from le-byte should be ok");

        let sign = sk.sign(data)
            .expect("sign should be ok");

        let pk = PublicKey::try_from(&sk)
            .expect("to public key should be ok");

        let _ = pk.verify(data, &sign)
            .expect("verify should be ok");
    }

    #[test]
    fn test_p256_verify() {
        let data = b"hello world";
        let pk = "031f64da8a38e6c1e5423a72ddd6d4fc4a777abe537e5cb5aa0425685cda8e063b"
            .decode_hex()
            .expect("hex decode should be ok");

        let pk = PublicKey::from_compressed(pk.as_slice())
            .expect("to public key should be ok");

        let sign = "b1855cec16b6ebb372895d44c7be3832b81334394d80bec7c4f00a9c1d9c3237\
        541834638d11ad9c62792ed548c9602c1d8cd0ca92fdd5e68ceea40e7bcfbeb2"
            .decode_hex()
            .expect("hex decode should be ok");

        let _ = pk.verify_digest(data, sign.as_slice())
            .expect("verify should be ok");

        let _ = pk.verify_digest(data, "b1855cec16b6ebb372895d44c7be3832")
            .expect_err("verify should be fail");
    }
}

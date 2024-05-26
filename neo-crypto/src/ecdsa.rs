// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use p256::ecdsa::signature::{Signer, Verifier as P256Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use crate::ecc256;

pub const ECC256_SIGN_SIZE: usize = 32 * 2;

pub trait Secp256r1Sign {
    type Sign;
    type Error;

    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<Self::Sign, Self::Error>;
}

pub trait Secp256r1Verify {
    type Sign;
    type Error;

    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        sign: &Self::Sign,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SignError {
    #[error("ecdsa: invalid private key")]
    InvalidKey,
}

impl Secp256r1Sign for ecc256::PrivateKey {
    type Sign = [u8; ECC256_SIGN_SIZE];
    type Error = SignError;

    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<Self::Sign, Self::Error> {
        let sk: SigningKey = p256::SecretKey::from_slice(self.as_be_bytes())
            .map(|key| key.into())
            .map_err(|_err| SignError::InvalidKey)?;

        let sign: Signature = sk
            .try_sign(message.as_ref())
            .map_err(|_err| SignError::InvalidKey)?;

        Ok(sign.to_bytes().into()) // big endian
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VerifyError {
    #[error("ecdsa: invalid public key")]
    InvalidKey,

    #[error("ecdsa: invalid sign")]
    InvalidSign,
}

impl Secp256r1Verify for ecc256::PublicKey {
    type Sign = [u8; ECC256_SIGN_SIZE];
    type Error = VerifyError;

    #[inline]
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        sign: &Self::Sign,
    ) -> Result<(), Self::Error> {
        let sign = Signature::try_from(sign.as_ref()).map_err(|_err| VerifyError::InvalidSign)?;
        VerifyingKey::from_sec1_bytes(&self.to_uncompressed())
            .map_err(|_err| VerifyError::InvalidKey)?
            .verify(message.as_ref(), &sign)
            .map_err(|_err| VerifyError::InvalidSign)
    }
}

use alloc::fmt;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex};

use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::SecretKey;

use crate::ecc256;

pub const SIGNATURE_SIZE: usize = 64;

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct SignatureBytes(pub [u8; SIGNATURE_SIZE]);

impl SignatureBytes {
    #[inline]
    pub fn as_ref(&self) -> &[u8; SIGNATURE_SIZE] {
        &self.0
    }
}

impl fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SignatureBytes")
            .field(&self.0.to_hex_lower())
            .finish()
    }
}

impl NeoEncode for SignatureBytes {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for SignatureBytes {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; SIGNATURE_SIZE];
        reader.read_into(&mut buf)?;
        Ok(SignatureBytes(buf))
    }
}

impl TryFrom<SignatureBytes> for Signature {
    type Error = VerifyError;

    #[inline]
    fn try_from(value: SignatureBytes) -> Result<Self, Self::Error> {
        Signature::try_from(value.0.as_slice()).map_err(|_| VerifyError::InvalidSignature)
    }
}

impl From<Signature> for SignatureBytes {
    #[inline]
    fn from(value: Signature) -> Self {
        SignatureBytes(value.to_bytes().into())
    }
}

pub trait Secp256r1Sign {
    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<SignatureBytes, SignError>;
}

pub trait Secp256r1Verify {
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        signature: &SignatureBytes,
    ) -> Result<(), VerifyError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SignError {
    #[error("ecdsa: invalid private key")]
    InvalidKey,

    #[error("ecdsa: signing failed")]
    SigningFailed,
}

impl Secp256r1Sign for ecc256::PrivateKey {
    #[inline]
    fn secp256r1_sign<T: AsRef<[u8]>>(&self, message: T) -> Result<SignatureBytes, SignError> {
        let secret =
            SecretKey::from_slice(self.as_be_bytes()).map_err(|_| SignError::InvalidKey)?;
        let signing_key: SigningKey = secret.into();
        let signature: Signature = signing_key
            .try_sign(message.as_ref())
            .map_err(|_| SignError::SigningFailed)?;
        Ok(signature.into())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VerifyError {
    #[error("ecdsa: invalid public key")]
    InvalidKey,

    #[error("ecdsa: invalid signature")]
    InvalidSignature,
}

impl Secp256r1Verify for ecc256::PublicKey {
    #[inline]
    fn secp256r1_verify<T: AsRef<[u8]>>(
        &self,
        message: T,
        signature: &SignatureBytes,
    ) -> Result<(), VerifyError> {
        let signature = Signature::try_from(*signature)?;
        let verifying = VerifyingKey::from_sec1_bytes(&self.to_uncompressed())
            .map_err(|_| VerifyError::InvalidKey)?;
        verifying
            .verify(message.as_ref(), &signature)
            .map_err(|_| VerifyError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn sign_and_verify() {
        let sk_bytes = hex!("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b848a7d84b8b620");
        let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
        let secret = SecretKey::from_slice(&sk_bytes).unwrap();
        let signing = SigningKey::from(secret.clone());
        let public_encoded = signing.verifying_key().to_encoded_point(true);
        let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();
        let message = b"neo-n3";

        let signature = private.secp256r1_sign(message).unwrap();
        public.secp256r1_verify(message, &signature).unwrap();
    }
}

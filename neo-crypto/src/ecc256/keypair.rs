use p256::{
    elliptic_curve::rand_core::{CryptoRng, RngCore},
    AffinePoint, PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};

use super::{private::PrivateKey, public::PublicKey};
use p256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};

#[derive(Clone)]
pub struct Keypair {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
}

#[derive(Debug, Copy, Clone, thiserror::Error)]
pub enum KeyError {
    #[error("ecc256: invalid private key length")]
    InvalidPrivateKeyLength,

    #[error("ecc256: invalid public key encoding")]
    InvalidPublicKey,
}

impl Keypair {
    #[inline]
    pub fn new(private_key: PrivateKey, public_key: PublicKey) -> Self {
        Self {
            private_key,
            public_key,
        }
    }

    #[inline]
    pub fn from_private(private_key: PrivateKey) -> Result<Self, KeyError> {
        let secret = P256SecretKey::from_slice(private_key.as_be_bytes())
            .map_err(|_| KeyError::InvalidPrivateKeyLength)?;
        let public = p256_public_to_inner(secret.public_key());
        Ok(Self {
            public_key: public,
            private_key,
        })
    }

    pub fn generate<R: CryptoRng + RngCore>(rng: &mut R) -> Self {
        let secret = P256SecretKey::random(rng);
        let private = PrivateKey::new(secret.to_bytes().into());
        let public = p256_public_to_inner(secret.public_key());
        Self {
            private_key: private,
            public_key: public,
        }
    }
}

fn p256_public_to_inner(public: P256PublicKey) -> PublicKey {
    let encoded = public.to_encoded_point(false);
    let affine = Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&encoded))
        .expect("p256 public key must decode into affine point");
    PublicKey::from_affine(affine)
}

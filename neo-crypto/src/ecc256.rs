// Copyright @ 2025 - present, R3E Network
// All Rights Reserved
use alloc::fmt::{self, Debug, Formatter};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex},
    hash::{hash160, Hash160},
};

use p256::{
    elliptic_curve::{
        rand_core::{CryptoRng, RngCore},
        sec1::{FromEncodedPoint, ToEncodedPoint},
    },
    AffinePoint, EncodedPoint, PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const KEY_SIZE: usize = 32;

#[derive(Clone)]
pub struct PrivateKey {
    key: Zeroizing<[u8; KEY_SIZE]>,
}

impl PrivateKey {
    #[inline]
    pub fn new(bytes: [u8; KEY_SIZE]) -> Self {
        Self {
            key: Zeroizing::new(bytes),
        }
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, KeyError> {
        if slice.len() != KEY_SIZE {
            return Err(KeyError::InvalidPrivateKeyLength);
        }
        let mut buf = [0u8; KEY_SIZE];
        buf.copy_from_slice(slice);
        Ok(Self::new(buf))
    }

    #[inline]
    pub fn as_be_bytes(&self) -> &[u8] {
        self.key.as_slice()
    }
}

impl Debug for PrivateKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PrivateKey").field(&"***").finish()
    }
}

impl Eq for PrivateKey {}

impl PartialEq for PrivateKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key.as_slice().ct_eq(other.key.as_slice()).into()
    }
}

impl PartialEq<[u8]> for PrivateKey {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.key.as_slice().ct_eq(other).into()
    }
}

impl NeoEncode for PrivateKey {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(self.as_be_bytes());
    }
}

impl NeoDecode for PrivateKey {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; KEY_SIZE];
        reader.read_into(&mut buf)?;
        Ok(PrivateKey::new(buf))
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct PublicKey {
    gx: [u8; KEY_SIZE],
    gy: [u8; KEY_SIZE],
}

impl PublicKey {
    #[inline]
    pub fn from_affine(point: AffinePoint) -> Self {
        let encoded = point.to_encoded_point(false);
        let mut gx = [0u8; KEY_SIZE];
        let mut gy = [0u8; KEY_SIZE];
        let x = encoded.x().expect("x coordinate");
        let y = encoded.y().expect("y coordinate");
        gx.copy_from_slice(x.as_ref());
        gy.copy_from_slice(y.as_ref());
        Self { gx, gy }
    }

    #[inline]
    pub fn from_sec1_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        let encoded = EncodedPoint::from_bytes(bytes).map_err(|_| KeyError::InvalidPublicKey)?;
        let point = Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&encoded))
            .ok_or(KeyError::InvalidPublicKey)?;
        Ok(Self::from_affine(point))
    }

    #[inline]
    pub fn to_uncompressed(&self) -> [u8; 65] {
        let mut buf = [0u8; 65];
        buf[0] = 0x04;
        buf[1..33].copy_from_slice(&self.gx);
        buf[33..].copy_from_slice(&self.gy);
        buf
    }

    #[inline]
    pub fn to_compressed(&self) -> [u8; 33] {
        let mut buf = [0u8; 33];
        buf[0] = 0x02 + (self.gy[KEY_SIZE - 1] & 0x01);
        buf[1..].copy_from_slice(&self.gx);
        buf
    }

    #[inline]
    pub fn script_hash(&self) -> Hash160 {
        Hash160::from_slice(&hash160(self.to_compressed())).expect("hash160 length is 20")
    }
}

impl Debug for PublicKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicKey")
            .field("compressed", &self.to_compressed().to_hex_lower())
            .finish()
    }
}

impl NeoEncode for PublicKey {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let compressed = self.to_compressed();
        writer.write_var_bytes(&compressed);
    }
}

impl NeoDecode for PublicKey {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let bytes = reader.read_var_bytes(65)?;
        PublicKey::from_sec1_bytes(&bytes).map_err(|_| DecodeError::InvalidValue("PublicKey"))
    }
}

#[derive(Clone)]
pub struct Keypair {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
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

#[derive(Debug, Copy, Clone, thiserror::Error)]
pub enum KeyError {
    #[error("ecc256: invalid private key length")]
    InvalidPrivateKeyLength,

    #[error("ecc256: invalid public key encoding")]
    InvalidPublicKey,
}

fn p256_public_to_inner(public: P256PublicKey) -> PublicKey {
    let encoded = public.to_encoded_point(false);
    let affine = Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&encoded))
        .expect("p256 public key must decode into affine point");
    PublicKey::from_affine(affine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use hex_literal::hex;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn compressed_roundtrip() {
        let sk = PrivateKey::new(hex!(
            "c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75"
        ));
        let pk = PublicKey::from_sec1_bytes(&hex!(
            "026e4bd1ab2b358fa6afa7e7f61f1c5d6b1fbcf91f55c2e1e7dda3297e4a8bba03"
        ))
        .unwrap();

        let mut buf = Vec::new();
        pk.neo_encode(&mut buf);
        let mut reader = neo_base::encoding::SliceReader::new(buf.as_slice());
        let decoded = PublicKey::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, pk);
        assert_eq!(sk.as_be_bytes().len(), KEY_SIZE);
        assert_ne!(pk.gx, [0u8; KEY_SIZE]);
    }

    #[test]
    fn keypair_from_private_matches_public() {
        let private = PrivateKey::from_slice(&[0x11; KEY_SIZE]).unwrap();
        let keypair = Keypair::from_private(private.clone()).unwrap();
        let derived = PublicKey::from_sec1_bytes(&keypair.public_key.to_compressed()).unwrap();
        assert_eq!(derived, keypair.public_key);
        assert_eq!(keypair.private_key, private);
    }

    #[test]
    fn keypair_generate_produces_valid_keys() {
        let mut rng = StdRng::seed_from_u64(7);
        let keypair = Keypair::generate(&mut rng);
        let public = &keypair.public_key;
        let compressed = public.to_compressed();
        let decoded = PublicKey::from_sec1_bytes(&compressed).unwrap();
        assert_eq!(decoded, *public);
        assert_eq!(keypair.private_key.as_be_bytes().len(), KEY_SIZE);
    }
}

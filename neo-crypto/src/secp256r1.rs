// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};
use core::{result::Result, convert::TryFrom};

use p256::elliptic_curve::{sec1::ToEncodedPoint};
use serde::{Serializer, Serialize, Deserializer, Deserialize, de::Error};

use neo_base::{errors, bytes::{ToRevArray, ToArray}, encoding::hex::{FromHex, ToHex}, encoding::bin::*};
use crate::{rand, key::SecretKey};


pub const KEY_SIZE: usize = 32;
pub const PUBLIC_COMPRESSED_SIZE: usize = KEY_SIZE + 1;

const ODD_PREFIX: u8 = 0x03;
const EVEN_PREFIX: u8 = 0x02;
const UNCOMPRESSED_PREFIX: u8 = 0x04;


pub struct Keypair<'a> {
    pub secret: &'a PrivateKey,
    pub public: &'a PublicKey,
}

impl<'a> Keypair<'a> {
    pub fn new(secret: &'a PrivateKey, public: &'a PublicKey) -> Self {
        Self { secret, public }
    }
}

/// little endian. PrivateKey i.e. SecretKey. sk -> SecretKey, pk -> PublicKey
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PrivateKey {
    key: SecretKey<KEY_SIZE>,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum DecodePrivateKeyError {
    #[error("secp256r1: invalid private key")]
    InvalidPrivateKey,
}

impl PrivateKey {
    #[inline]
    pub fn as_le_bytes(&self) -> &[u8] { self.key.as_ref() }

    pub fn from_le_bytes(sk: &[u8]) -> Result<Self, DecodePrivateKeyError> {
        use DecodePrivateKeyError as Error;

        if sk.len() != KEY_SIZE {
            return Err(Error::InvalidPrivateKey);
        }

        let sk: [u8; KEY_SIZE] = sk.to_rev_array();
        let point = p256::SecretKey::from_slice(sk.as_slice())
            .map_err(|_| Error::InvalidPrivateKey)?;

        Ok(Self { key: point.to_bytes().as_slice().to_rev_array().into() })
    }

    pub fn from_be_bytes(sk: &[u8]) -> Result<Self, DecodePrivateKeyError> {
        use DecodePrivateKeyError as Error;

        if sk.len() != KEY_SIZE {
            return Err(Error::InvalidPrivateKey);
        }

        let sk: [u8; KEY_SIZE] = sk.to_array();
        let point = p256::SecretKey::from_slice(sk.as_slice())
            .map_err(|_| Error::InvalidPrivateKey)?;

        Ok(Self { key: point.to_bytes().as_slice().to_rev_array().into() })
    }
}

/// little endian
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PublicKey {
    gx: [u8; KEY_SIZE],
    gy: [u8; KEY_SIZE],
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum FromPrivateKeyError {
    #[error("secp256r1: invalid private key")]
    InvalidPrivateKey,
}

impl TryFrom<&PrivateKey> for PublicKey {
    type Error = FromPrivateKeyError;

    fn try_from(sk: &PrivateKey) -> Result<Self, Self::Error> {
        use FromPrivateKeyError as Error;

        let sk = sk.key.reverse();
        let point = p256::SecretKey::from_slice(sk.as_ref())
            .map_err(|_| Error::InvalidPrivateKey)?
            .public_key()
            .to_encoded_point(false);

        let x = point.x().ok_or(Error::InvalidPrivateKey)?;
        let y = point.y().ok_or(Error::InvalidPrivateKey)?;
        Ok(PublicKey {
            gx: x.as_slice().to_rev_array(),
            gy: y.as_slice().to_rev_array(),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, errors::Error)]
pub enum DecodePublicKeyError {
    #[error("secp256r1: invalid public key")]
    InvalidPublicKey,
}

impl PublicKey {
    #[inline]
    pub fn as_le_bytes(&self) -> (&[u8], &[u8]) {
        (self.gx.as_slice(), self.gy.as_slice())
    }

    pub fn to_uncompressed(&self) -> [u8; 2 * KEY_SIZE + 1] {
        let mut buf = [0u8; 65];
        let (gx, gy) = self.as_le_bytes();

        buf[0] = UNCOMPRESSED_PREFIX;
        buf[1..1 + KEY_SIZE].copy_from_slice(gx);
        buf[1 + KEY_SIZE..].copy_from_slice(gy);

        buf[1..1 + KEY_SIZE].reverse();
        buf[1 + KEY_SIZE..].reverse();

        buf
    }

    pub fn to_compressed(&self) -> [u8; KEY_SIZE + 1] {
        let mut buf = [0u8; 33];
        let (gx, gy) = self.as_le_bytes();

        buf[0] = if gy[0] & 0x01 == 0 { EVEN_PREFIX } else { ODD_PREFIX };
        buf[1..].copy_from_slice(gx);
        buf[1..].reverse();

        buf
    }

    pub fn from_compressed(key: &[u8]) -> Result<Self, DecodePublicKeyError> {
        Self::from_sec1_bytes(key)
    }

    pub fn from_uncompressed(key: &[u8]) -> Result<Self, DecodePublicKeyError> {
        Self::from_sec1_bytes(key)
    }

    fn from_sec1_bytes(key: &[u8]) -> Result<Self, DecodePublicKeyError> {
        use DecodePublicKeyError as Error;

        let point = p256::PublicKey::from_sec1_bytes(key)
            .map_err(|_| Error::InvalidPublicKey)?
            .to_encoded_point(false);

        let x = point.x().ok_or(Error::InvalidPublicKey)?;
        let y = point.y().ok_or(Error::InvalidPublicKey)?;
        if x.len() != KEY_SIZE || y.len() != KEY_SIZE {
            return Err(Error::InvalidPublicKey);
        }

        Ok(PublicKey {
            gx: x.as_slice().to_rev_array(),
            gy: y.as_slice().to_rev_array(),
        })
    }
}

impl FromHex for PublicKey {
    type Error = DecodePublicKeyError;

    #[inline]
    fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        let v = <[u8; PUBLIC_COMPRESSED_SIZE]>::from_hex(hex)
            .map_err(|_err| Self::Error::InvalidPublicKey)?;

        PublicKey::from_compressed(v.as_slice())
    }
}

impl Serialize for PublicKey {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_compressed().to_hex())
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PublicKey::from_hex(String::deserialize(deserializer)?.as_str())
            .map_err(D::Error::custom)
    }
}

impl BinEncoder for PublicKey {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        let pk = self.to_compressed();
        w.write(pk.as_slice());
    }

    fn bin_size(&self) -> usize { PUBLIC_COMPRESSED_SIZE }
}

impl BinDecoder for PublicKey {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let offset = r.consumed();
        let b = u8::decode_bin(r)?;
        match b {
            ODD_PREFIX | EVEN_PREFIX => {
                let mut pk = [0u8; PUBLIC_COMPRESSED_SIZE];

                pk[0] = b;
                r.read_full(&mut pk[1..])?;
                PublicKey::from_compressed(&pk)
                    .map_err(|_| BinDecodeError::InvalidValue("PublicKey", offset))
            }
            UNCOMPRESSED_PREFIX => {
                let mut pk = [0u8; KEY_SIZE * 2];

                r.read_full(&mut pk)?;
                PublicKey::from_uncompressed(&pk)
                    .map_err(|_| BinDecodeError::InvalidValue("PublicKey", offset))
            }
            _ => Err(BinDecodeError::InvalidType("PublicKey", offset, b as u64))
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, errors::Error)]
pub enum GenKeyError {
    #[error("secp256r1: gen random error {0}")]
    GenRandomError(String),
}


pub trait GenKeypair {
    fn gen_keypair(&mut self) -> Result<(PrivateKey, PublicKey), GenKeyError>;
}

fn gen_private_key(random: &mut impl rand::CryptoRand) -> Result<p256::SecretKey, GenKeyError> {
    use p256::elliptic_curve::PrimeField;

    let mut bytes = p256::FieldBytes::default();
    for _ in 0..5 {
        random.read_full(bytes.as_mut_slice())
            .map_err(|err| GenKeyError::GenRandomError(err.to_string()))?;

        let no_zero: Option<p256::NonZeroScalar> = p256::Scalar::from_repr(bytes)
            .and_then(|scalar| p256::NonZeroScalar::new(scalar))
            .into();
        if let Some(key) = no_zero { // TODO: CtOption
            return Ok(p256::SecretKey::new(key.into()));
        }
    }

    Err(GenKeyError::GenRandomError("try too many times".into()))
}


impl<T: rand::CryptoRand> GenKeypair for T {
    fn gen_keypair(&mut self) -> Result<(PrivateKey, PublicKey), GenKeyError> {
        let sk = gen_private_key(self)?;

        let pk = sk.public_key().to_encoded_point(false);
        let gx = pk.x().ok_or(GenKeyError::GenRandomError("unexpected x".into()))?;
        let gy = pk.y().ok_or(GenKeyError::GenRandomError("unexpected y".into()))?;

        let sk = sk.to_bytes();
        if sk.len() != KEY_SIZE || gx.len() != KEY_SIZE || gy.len() != KEY_SIZE {
            return Err(GenKeyError::GenRandomError("unexpected key size".into()));
        }

        let key = sk.as_slice().to_rev_array();
        let gx = gx.as_slice().to_rev_array();
        let gy = gy.as_slice().to_rev_array();

        Ok((PrivateKey { key: key.into() }, PublicKey { gx, gy }))
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use neo_base::encoding::hex::{DecodeHex, ToHex};
    use crate::rand::OsRand;

    #[test]
    fn test_from_compressed() {
        let x = "035a928f201639204e06b4368b1a93365462a8ebbff0b8818151b74faab3a2b61a"
            .decode_hex()
            .expect("hex decode should be ok");

        let key = PublicKey::from_compressed(&x)
            .expect("from compressed public should be ok");

        let gy: [u8; KEY_SIZE] = key.gy.as_slice().to_rev_array();
        assert_eq!("35dfabcb79ac492a2a88588d2f2e73f045cd8af58059282e09d693dc340e113f", &gy.to_hex());
        assert_eq!(x.as_slice(), key.to_compressed().as_slice());

        let uncompressed = key.to_uncompressed();
        let got = PublicKey::from_uncompressed(&uncompressed)
            .expect("from uncompressed should be ok");
        assert_eq!(key, got);

        let should = "035a928f201639204e06b4368b1a93365462a8ebbff0b8818151b74faab3a2b61a"
            .decode_hex()
            .expect("decode key should be ok");

        assert_eq!(key.to_compressed().as_slice(), should.as_slice());
    }

    #[test]
    fn test_gen_keypair() {
        for _ in 0..10 {
            let (sk, pk) = OsRand::gen_keypair(&mut OsRand)
                .expect("gen_keypair should be ok");

            let got = PublicKey::from_compressed(pk.to_compressed().as_slice())
                .expect("from compressed public should be ok");
            assert_eq!(pk, got);

            let from_sk = PublicKey::try_from(&sk)
                .expect("try from sk should be ok");
            assert_eq!(pk, from_sk);
        }
    }
}
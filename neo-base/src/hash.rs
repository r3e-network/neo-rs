use alloc::{
    fmt::{self, Display, Formatter},
    format,
    string::String,
};
use core::ops::Deref;

use crate::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use sha2::Digest;

/// Compute a single round SHA-256 hash over the provided bytes.
#[inline]
pub fn sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(data.as_ref());
    h.finalize().into()
}

/// Compute two rounds of SHA-256 – the default block hashing strategy in Neo.
#[inline]
pub fn double_sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    sha256(sha256(data))
}

/// Compute RIPEMD-160 hash.
#[inline]
pub fn ripemd160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let mut ripemd = ripemd::Ripemd160::new();
    ripemd.update(data.as_ref());
    ripemd.finalize().into()
}

/// Compute RIPEMD-160(SHA-256(data)) which is used for script hashes.
#[inline]
pub fn hash160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let sha = sha256(data);
    ripemd160(sha)
}

/// Compute Keccak-256 hash.
#[inline]
pub fn keccak256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    use sha3::Digest as _;
    let mut hasher = sha3::Keccak256::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

/// Hash160 is the canonical type for Neo script hashes (20 bytes).
#[derive(Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord, Default, Debug)]
pub struct Hash160(pub [u8; 20]);

impl Hash160 {
    pub const ZERO: Self = Self([0u8; 20]);

    #[inline]
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != 20 {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: 20,
            });
        }

        let mut buf = [0u8; 20];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }
}

impl AsRef<[u8]> for Hash160 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for Hash160 {
    type Target = [u8; 20];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Hash160 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl Serialize for Hash160 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hash160 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let decoded = decode_hex_string::<D, 20>(&value)?;
        Ok(Hash160::new(decoded))
    }
}

impl NeoEncode for Hash160 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for Hash160 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; 20];
        reader.read_into(&mut buf)?;
        Ok(Hash160(buf))
    }
}

/// Hash256 represents double-SHA256 digests (32 bytes).
#[derive(Clone, Copy, Eq, PartialEq, Hash, Default, Debug)]
pub struct Hash256(pub [u8; 32]);

impl Hash256 {
    pub const ZERO: Self = Self([0u8; 32]);

    #[inline]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != 32 {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: 32,
            });
        }

        let mut buf = [0u8; 32];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }
}

impl AsRef<[u8]> for Hash256 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for Hash256 {
    type Target = [u8; 32];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Hash256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl Serialize for Hash256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hash256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let decoded = decode_hex_string::<D, 32>(&value)?;
        Ok(Hash256::new(decoded))
    }
}

impl From<[u8; 32]> for Hash256 {
    #[inline]
    fn from(value: [u8; 32]) -> Self {
        Self(value)
    }
}

impl NeoEncode for Hash256 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for Hash256 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; 32];
        reader.read_into(&mut buf)?;
        Ok(Hash256(buf))
    }
}

fn decode_hex_string<'de, D: Deserializer<'de>, const N: usize>(
    value: &str,
) -> Result<[u8; N], D::Error> {
    let trimmed = value.trim_start_matches("0x");
    let decoded = hex::decode(trimmed).map_err(DeError::custom)?;
    if decoded.len() != N {
        return Err(DeError::custom(format!(
            "expected {} bytes, got {}",
            N,
            decoded.len()
        )));
    }
    let mut array = [0u8; N];
    array.copy_from_slice(&decoded);
    Ok(array)
}

// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};
use core::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use neo_base::encoding::{BinEncoder, BinWriter, StartsWith0x, ToRevHex};

pub const H256_SIZE: usize = 32;

/// little endian
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct H256([u8; H256_SIZE]);

impl H256 {
    pub fn from_le_bytes(src: [u8; H256_SIZE]) -> Self {
        H256(src)
    }

    pub fn as_le_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; H256_SIZE]> for H256 {
    #[inline]
    fn as_ref(&self) -> &[u8; H256_SIZE] {
        &self.0
    }
}

impl AsRef<[u8]> for H256 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        return &self.0;
    }
}

impl Display for H256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("0x")?;
        f.write_str(&self.0.to_rev_hex_lower())
    }
}

impl Default for H256 {
    #[inline]
    fn default() -> Self {
        Self([0u8; H256_SIZE])
    }
}

impl BinEncoder for H256 {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.0);
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ToH256Error {
    #[error("to-h256: hex-encode H256's length must be 64(without '0x')")]
    InvalidLength,

    #[error("to-h256: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for H256 {
    type Error = ToH256Error;

    /// value must be big-endian
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() {
            &value[2..]
        } else {
            value
        };

        let mut buf = [0u8; H256_SIZE];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|err| match err {
            hex::FromHexError::OddLength | hex::FromHexError::InvalidStringLength => {
                Self::Error::InvalidLength
            }
            hex::FromHexError::InvalidHexCharacter { c, index: _ } => Self::Error::InvalidChar(c),
        })?;

        buf.reverse();
        Ok(Self(buf))
    }
}

impl Serialize for H256 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for H256 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        H256::try_from(value.as_str()).map_err(D::Error::custom)
    }
}
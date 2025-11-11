use alloc::{
    fmt::{self, Display, Formatter},
    string::{String, ToString},
};
use core::{cmp::Ordering, str::FromStr};

use crate::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToRevHex};

use super::model::{UInt256, U256_LEN};

impl AsRef<[u8]> for UInt256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<[u8; U256_LEN]> for UInt256 {
    #[inline]
    fn from(value: [u8; U256_LEN]) -> Self {
        Self(value)
    }
}

impl From<UInt256> for [u8; U256_LEN] {
    #[inline]
    fn from(value: UInt256) -> Self {
        value.0
    }
}

impl Ord for UInt256 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}

impl PartialOrd for UInt256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for UInt256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", self.0.to_rev_hex_upper())
    }
}

impl fmt::Debug for UInt256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl FromStr for UInt256 {
    type Err = DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex_str(s)
    }
}

impl NeoEncode for UInt256 {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for UInt256 {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; U256_LEN];
        reader.read_into(&mut buf)?;
        Ok(Self(buf))
    }
}

impl serde::Serialize for UInt256 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for UInt256 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::from_hex_str(&value).map_err(serde::de::Error::custom)
    }
}

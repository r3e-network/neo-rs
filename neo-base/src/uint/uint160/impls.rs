use alloc::{
    fmt::{self, Display, Formatter},
    string::{String, ToString},
};
use core::{cmp::Ordering, str::FromStr};

use crate::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToRevHex};

use super::model::{UInt160, U160_LEN};

impl AsRef<[u8]> for UInt160 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<[u8; U160_LEN]> for UInt160 {
    #[inline]
    fn from(value: [u8; U160_LEN]) -> Self {
        Self(value)
    }
}

impl From<UInt160> for [u8; U160_LEN] {
    #[inline]
    fn from(value: UInt160) -> Self {
        value.0
    }
}

impl Ord for UInt160 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}

impl PartialOrd for UInt160 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for UInt160 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", self.0.to_rev_hex_upper())
    }
}

impl fmt::Debug for UInt160 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl FromStr for UInt160 {
    type Err = DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex_str(s)
    }
}

impl NeoEncode for UInt160 {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for UInt160 {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; U160_LEN];
        reader.read_into(&mut buf)?;
        Ok(Self(buf))
    }
}

impl serde::Serialize for UInt160 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for UInt160 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::from_hex_str(&value).map_err(serde::de::Error::custom)
    }
}

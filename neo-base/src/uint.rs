use alloc::{
    fmt::{self, Display, Formatter},
    string::{String, ToString},
    vec::Vec,
};
use core::{cmp::Ordering, str::FromStr};

use crate::encoding::{
    DecodeError, FromBase58Check, FromBase58CheckError, NeoDecode, NeoEncode, NeoRead, NeoWrite,
    StartsWith0x, ToBase58Check, ToRevHex,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum AddressError {
    #[error("address: invalid length {length}, expected 21 bytes (version + script hash)")]
    InvalidLength { length: usize },

    #[error("address: invalid version byte (expected 0x{expected:02X}, found 0x{found:02X})")]
    InvalidVersion { expected: u8, found: u8 },

    #[error("address: {0}")]
    Base58(#[from] FromBase58CheckError),
}

/// Wrapper around the Neo protocol address version byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddressVersion(pub u8);

impl AddressVersion {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Mainnet address version (0x35 in C# `ProtocolSettings`).
    pub const MAINNET: Self = Self(0x35);

    /// Testnet address version (0x23 in C# `ProtocolSettings`).
    pub const TESTNET: Self = Self(0x23);
}

impl Default for AddressVersion {
    fn default() -> Self {
        Self::MAINNET
    }
}

const U160_LEN: usize = 20;
const U256_LEN: usize = 32;

/// Neo's 160-bit unsigned integer (little-endian in memory, big-endian when formatted).
#[derive(Clone, Copy, Eq, PartialEq, Hash, Default)]
pub struct UInt160([u8; U160_LEN]);

impl UInt160 {
    pub const LENGTH: usize = U160_LEN;
    pub const ZERO: Self = Self([0u8; U160_LEN]);

    #[inline]
    pub const fn new(bytes: [u8; U160_LEN]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != U160_LEN {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: U160_LEN as u64,
            });
        }
        let mut buf = [0u8; U160_LEN];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }

    #[inline]
    pub fn to_array(self) -> [u8; U160_LEN] {
        self.0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8; U160_LEN] {
        &self.0
    }

    #[inline]
    pub fn to_vec(self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn to_address(&self, version: AddressVersion) -> String {
        let mut payload = Vec::with_capacity(1 + U160_LEN);
        payload.push(version.0);
        payload.extend_from_slice(&self.0);
        payload.to_base58_check()
    }

    pub fn from_address(address: &str, version: AddressVersion) -> Result<Self, AddressError> {
        let decoded = Vec::<u8>::from_base58_check(address)?;
        if decoded.len() != 1 + U160_LEN {
            return Err(AddressError::InvalidLength {
                length: decoded.len(),
            });
        }
        if decoded[0] != version.0 {
            return Err(AddressError::InvalidVersion {
                expected: version.0,
                found: decoded[0],
            });
        }
        let mut buf = [0u8; U160_LEN];
        buf.copy_from_slice(&decoded[1..]);
        Ok(Self(buf))
    }

    #[inline]
    pub fn from_script(script: &[u8]) -> Self {
        let hash = crate::hash::hash160(script);
        Self::from_slice(&hash).expect("hash160 output size is 20 bytes")
    }

    pub fn from_hex_str(value: &str) -> Result<Self, DecodeError> {
        let trimmed = value.trim();
        let without_prefix = if trimmed.as_bytes().starts_with_0x() {
            &trimmed[2..]
        } else {
            trimmed
        };
        if without_prefix.len() != U160_LEN * 2 {
            return Err(DecodeError::LengthOutOfRange {
                len: without_prefix.len() as u64 / 2,
                max: U160_LEN as u64,
            });
        }
        let bytes =
            hex::decode(without_prefix).map_err(|_| DecodeError::InvalidValue("UInt160 hex"))?;
        let mut array = [0u8; U160_LEN];
        array.copy_from_slice(&bytes);
        array.reverse();
        Ok(Self(array))
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

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

/// Neo's 256-bit unsigned integer.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Default)]
pub struct UInt256([u8; U256_LEN]);

impl UInt256 {
    pub const LENGTH: usize = U256_LEN;
    pub const ZERO: Self = Self([0u8; U256_LEN]);

    #[inline]
    pub const fn new(bytes: [u8; U256_LEN]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != U256_LEN {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: U256_LEN as u64,
            });
        }
        let mut buf = [0u8; U256_LEN];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8; U256_LEN] {
        &self.0
    }

    #[inline]
    pub fn to_array(self) -> [u8; U256_LEN] {
        self.0
    }

    pub fn from_hex_str(value: &str) -> Result<Self, DecodeError> {
        let trimmed = value.trim();
        let without_prefix = if trimmed.as_bytes().starts_with_0x() {
            &trimmed[2..]
        } else {
            trimmed
        };
        if without_prefix.len() != U256_LEN * 2 {
            return Err(DecodeError::LengthOutOfRange {
                len: without_prefix.len() as u64 / 2,
                max: U256_LEN as u64,
            });
        }
        let bytes =
            hex::decode(without_prefix).map_err(|_| DecodeError::InvalidValue("UInt256 hex"))?;
        let mut array = [0u8; U256_LEN];
        array.copy_from_slice(&bytes);
        array.reverse();
        Ok(Self(array))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash160;
    use alloc::format;
    use alloc::vec;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde_json;

    #[test]
    fn display_matches_csharp_format() {
        let value = UInt160::from_slice(&[0x52; 20]).unwrap();
        assert_eq!(
            format!("{value}"),
            "0x5252525252525252525252525252525252525252"
        );
    }

    #[test]
    fn address_roundtrip_from_csharp_vectors() {
        let script = STANDARD
            .decode("DCECozKyXb9hGPwlv2Tw2DALu2I7eDRDcazwy1ByffMtnbNBVuezJw==")
            .expect("base64");
        let script_hash = UInt160::from_slice(&hash160(&script)).unwrap();
        let version = AddressVersion::MAINNET;
        let address = script_hash.to_address(version);
        assert_eq!(address, "NRPf2BLaP595UFybH1nwrExJSt5ZGbKnjd");
        let parsed = UInt160::from_address(&address, version).unwrap();
        assert_eq!(parsed, script_hash);
        assert_eq!(
            format!("{script_hash}"),
            "0x8618383E5B58C50C66BC8A8E8E43725DC41C153C"
        );
    }

    #[test]
    fn serde_roundtrip_uint256() {
        let value = UInt256::from_slice(&[0xAB; 32]).unwrap();
        let serialized = serde_json::to_string(&value).expect("serialize");
        let expected = format!("\"{value}\"");
        assert_eq!(serialized, expected);
        let deserialized: UInt256 = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, value);
    }

    #[test]
    fn parse_from_hex_string() {
        let original = UInt160::from_slice(&[0x01; 20]).unwrap();
        let encoded = original.to_string();
        let parsed = UInt160::from_hex_str(&encoded).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn address_errors_propagate() {
        let err = UInt160::from_address("not-an-address", AddressVersion::MAINNET).unwrap_err();
        assert!(matches!(err, AddressError::Base58(_)));

        let mut payload = vec![0x35];
        payload.extend_from_slice(&[0u8; 19]);
        let encoded = payload.to_base58_check();
        let err = UInt160::from_address(&encoded, AddressVersion::MAINNET).unwrap_err();
        assert!(matches!(err, AddressError::InvalidLength { .. }));
    }
}

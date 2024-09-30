// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::fmt::{Display, Formatter};

use neo_base::encoding::bin::*;
use neo_base::encoding::hex::{StartsWith0x, ToRevHex};
use neo_base::errors;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use crate::ToH160Error;

pub const H256_SIZE: usize = 32;

/// little endian
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct H256([u8; H256_SIZE]);

impl H256 {
    pub const LEN: usize = H256_SIZE;

    pub fn len() -> usize {
        H256_SIZE
    }
    pub fn from_script(p0: &Vec<u8>) -> H256 {
        let mut buf = [0u8; H256_SIZE];
        buf.copy_from_slice(p0);
        buf.reverse();
        H256::from(buf)
    }

    pub fn zero() -> Self {
        Self([0u8; H256_SIZE])
    }

    pub const ZERO: Self = Self::zero();

    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; H256_SIZE]
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
        &self.0
    }
}

impl From<[u8; H256_SIZE]> for H256 {
    /// NOTE: value is little endian.
    ///  if H256 is from sha256-hash, and the `to_string` will output a reversed hex-string from sha256-hash.
    fn from(value: [u8; H256_SIZE]) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for H256 {
    fn from(value: &[u8]) -> Self {
        let mut buf = [0u8; H256_SIZE];
        buf.copy_from_slice(value);
        buf.reverse();
        H256::from(buf)
    }
}

impl From<&str> for H256 {
    fn from(value: &str) -> Self {
        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() { &value[2..] } else { value };
        H256::try_from(value).expect("hex decode should be ok")
    }
}

impl Display for H256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("0x")?;
        f.write_str(&self.0.to_rev_hex())
    }
}

impl BinEncoder for H256 {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.0);
    }

    fn bin_size(&self) -> usize {
        H256_SIZE
    }
}

impl BinDecoder for H256 {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut h = H256([0u8; H256_SIZE]);
        r.read_full(h.0.as_mut_slice())?;

        Ok(h)
    }
}

#[derive(Debug, Clone, Copy, errors::Error)]
pub enum ToH256Error {
    #[error("to-h256: hex-encode H160's length must be 64(without '0x')")]
    InvalidLength,

    #[error("to-h256: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for H256 {
    type Error = ToH256Error;

    /// value must be big-endian
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use hex::FromHexError as HexError;

        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() { &value[2..] } else { value };

        let mut buf = [0u8; H256_SIZE];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|e| match e {
            HexError::OddLength | HexError::InvalidStringLength => ToH160Error::InvalidLength,
            HexError::InvalidHexCharacter { c: ch, index: _ } => ToH160Error::InvalidChar(ch),
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

impl Default for H256 {
    #[inline]
    fn default() -> Self {
        Self([0u8; H256_SIZE])
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_h256() {
        let h = "\"f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d\"";
        let h1 = H256::try_from(h).expect("hex decode should be ok");

        let x = serde_json::to_string(&h1).expect("json encode should be ok");
        assert_eq!(&h[1..], &x[3..]);
        assert_eq!(
            &h1.to_string(),
            "0xf037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d"
        );

        let h2: H256 = serde_json::from_str(h).expect("json decode should be ok");
        assert_eq!(h2, h1);

        let x = "0x1230000000000000000000000000000000000000000000000000000000000000";
        let h: H256 = x.try_into().expect("try_into should be ok");
        assert_eq!(x, &h.to_string());
    }
}

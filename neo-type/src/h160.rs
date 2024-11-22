// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::fmt::{Display, Formatter};
use std::{string::String, vec::Vec};

use neo_base::{
    encoding::bin::*,
    encoding::hex::{StartsWith0x, ToRevHex},
    errors,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

pub const H160_SIZE: usize = 20;

/// little endian
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct H160([u8; H160_SIZE]);

impl H160 {
    pub const LEN: usize = H160_SIZE;
    pub fn len() -> usize {
        H160_SIZE
    }
    pub fn from_script(p0: &Vec<u8>) -> H160 {
        let mut buf = [0u8; H160_SIZE];
        buf.copy_from_slice(p0);
        buf.reverse();
        H160::from(buf)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; H160_SIZE]
    }

    pub fn zero() -> Self {
        Self([0u8; H160_SIZE])
    }

    pub const ZERO: Self = Self::zero();

    pub fn as_le_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; H160_SIZE]> for H160 {
    #[inline]
    fn as_ref(&self) -> &[u8; H160_SIZE] {
        &self.0
    }
}

impl AsRef<[u8]> for H160 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<[u8; H160_SIZE]> for H160 {
    /// NOTE: value is little endian
    #[inline]
    fn from(value: [u8; H160_SIZE]) -> Self {
        Self(value)
    }
}

impl Into<[u8; H160_SIZE]> for H160 {
    #[inline]
    fn into(self) -> [u8; H160_SIZE] {
        self.0
    }
}

impl From<&[u8]> for H160 {
    fn from(value: &[u8]) -> Self {
        let mut buf = [0u8; H160_SIZE];
        buf.copy_from_slice(value);
        buf.reverse();
        H160::from(buf)
    }
}

impl From<&str> for H160 {
    fn from(value: &str) -> Self {
        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() { &value[2..] } else { value };
        H160::try_from(value).expect("hex decode should be ok")
    }
}

impl Display for H160 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("0x")?;
        f.write_str(&self.0.to_rev_hex())
    }
}

impl BinEncoder for H160 {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.0);
    }

    fn bin_size(&self) -> usize {
        H160_SIZE
    }
}

impl BinDecoder for H160 {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut h = H160([0u8; H160_SIZE]);
        r.read_full(h.0.as_mut_slice())?;

        Ok(h)
    }
}

#[derive(Debug, Clone, Copy, errors::Error)]
pub enum ToH160Error {
    #[error("to-h160: hex-encode H160's length must be 40(without '0x')")]
    InvalidLength,

    #[error("to-h160: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for H160 {
    type Error = ToH160Error;

    /// value must be big-endian
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use hex::FromHexError as HexError;

        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() { &value[2..] } else { value };

        let mut buf = [0u8; H160_SIZE];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|e| match e {
            HexError::OddLength | HexError::InvalidStringLength => ToH160Error::InvalidLength,
            HexError::InvalidHexCharacter { c, index: _ } => ToH160Error::InvalidChar(c),
        })?;

        buf.reverse();
        Ok(Self(buf))
    }
}

impl Default for H160 {
    #[inline]
    fn default() -> Self {
        Self([0u8; H160_SIZE])
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_h160() {
        let h = "\"0263c1de100292813b5e075e585acc1bae963b2d\"";
        let h1 = H160::try_from(h).expect("hex decode should be ok");

        let x = serde_json::to_string(&h1).expect("json encode should be ok");
        assert_eq!(&h[1..], &x[3..]);
        assert_eq!(&h1.to_string(), "0x0263c1de100292813b5e075e585acc1bae963b2d");

        let h2: H160 = serde_json::from_str(h).expect("json decode should be ok");
        assert_eq!(h2, h1);
    }
}

// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};
use core::cmp::{Ord, Ordering, PartialOrd};
use core::fmt::{Debug, Display, Formatter};
use core::ops::{BitAnd, BitOr, BitXor, Not};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use crate::encoding::{bin::*, hex::StartsWith0x};
use crate::{bytes::ToRevArray, errors};

#[derive(Copy, Clone, Default, Hash, Eq, PartialEq)]
#[repr(C)]
pub struct I256 {
    low:  u128,
    high: i128,
}

impl I256 {
    pub const ZERO: Self = Self { low: 0, high: 0 };

    pub const ONE: Self = Self { low: 1, high: 0 };

    pub const MINUS_ONE: Self = Self { low: u128::MAX, high: -1 };

    pub const MAX: Self = Self { low: u128::MAX, high: i128::MAX };

    pub const MIN: Self = Self { low: u128::MIN, high: i128::MIN };

    pub const I32_MAX: Self = Self { low: i32::MAX as u128, high: 0 };

    pub const I32_MIN: Self = Self { low: u128::MIN, high: i32::MIN as i128 };

    // NOTE: assume platform endian is little endian
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; 32] {
        unsafe { core::mem::transmute_copy(self) }
    }

    #[inline]
    pub fn to_be_bytes(&self) -> [u8; 32] {
        self.to_le_bytes().to_rev_array()
    }

    #[inline]
    pub fn from_le_bytes(buf: [u8; 32]) -> Self {
        unsafe { core::mem::transmute_copy(&buf) }
    }

    #[inline]
    pub fn from_be_bytes(buf: [u8; 32]) -> Self {
        Self::from_le_bytes(buf.to_rev_array())
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.eq(&I256::ZERO)
    }

    #[inline]
    pub fn is_even(&self) -> bool {
        self.low & 1u128 == 0
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        self.high.is_negative()
    }

    #[inline]
    pub fn is_postive(&self) -> bool {
        self.high.is_positive() || (self.high == 0 && self.low != 0)
    }

    pub fn sign(&self) -> i8 {
        if self.is_zero() {
            0
        } else if self.is_negative() {
            -1
        } else {
            1
        }
    }

    #[inline]
    pub fn as_i128(&self) -> i128 {
        self.low as i128
    }

    #[inline]
    pub fn as_u128(&self) -> u128 {
        self.low
    }
}

impl Debug for I256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for I256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let h = self.to_be_bytes();
        f.write_str("0x")?;
        f.write_str(&hex::encode(h))
    }
}

impl PartialOrd for I256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for I256 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.high.cmp(&other.high).then(self.low.cmp(&other.low))
    }
}

impl From<i8> for I256 {
    #[inline]
    fn from(value: i8) -> Self {
        Self::from(value as i128)
    }
}

impl From<i16> for I256 {
    #[inline]
    fn from(value: i16) -> Self {
        Self::from(value as i128)
    }
}

impl From<i32> for I256 {
    #[inline]
    fn from(value: i32) -> Self {
        Self::from(value as i128)
    }
}

impl From<i64> for I256 {
    #[inline]
    fn from(value: i64) -> Self {
        Self::from(value as i128)
    }
}

impl From<usize> for I256 {
    #[inline]
    fn from(value: usize) -> Self {
        Self::from(value as i128)
    }
}

impl From<i128> for I256 {
    #[inline]
    fn from(value: i128) -> Self {
        Self { low: value as u128, high: value >> 127 }
    }
}

#[derive(Debug, Clone, Copy, errors::Error)]
pub enum ToI256Error {
    #[error("to-i256: hex-encode i256's length must be 64")]
    InvalidLength,

    #[error("to-i256: invalid character {0}")]
    InvalidChar(char),
}

impl TryFrom<&str> for I256 {
    type Error = ToI256Error;

    // FIXME: only hex string is supported now.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use hex::FromHexError as HexError;
        let value = if value.starts_with_0x() { &value[2..] } else { value };
        let mut buf = [0u8; 32];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|e| match e {
            HexError::OddLength | HexError::InvalidStringLength => Self::Error::InvalidLength,
            HexError::InvalidHexCharacter { c: ch, index: _ } => Self::Error::InvalidChar(ch),
        })?;

        Ok(Self::from_be_bytes(buf))
    }
}

impl Serialize for I256 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for I256 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        I256::try_from(value.as_str()).map_err(D::Error::custom)
    }
}

impl BinEncoder for I256 {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(self.to_le_bytes().as_slice());
    }

    fn bin_size(&self) -> usize {
        32
    }
}

impl BinDecoder for I256 {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut h = [0u8; 32];
        r.read_full(h.as_mut_slice())?;

        Ok(I256::from_le_bytes(h))
    }
}

impl BitAnd for I256 {
    type Output = I256;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self { low: self.low & rhs.low, high: self.high & rhs.high }
    }
}

impl BitOr for I256 {
    type Output = I256;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self { low: self.low | rhs.low, high: self.high | rhs.high }
    }
}

impl BitXor for I256 {
    type Output = I256;

    #[inline]
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self { low: self.low ^ rhs.low, high: self.high ^ rhs.high }
    }
}

impl Not for I256 {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self { low: !self.low, high: !self.high }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_i256_bitwise() {
        let buf = [1u8; 32];
        let n = I256::from_le_bytes(buf);
        assert_eq!(
            &n.to_string(),
            "0x0101010101010101010101010101010101010101010101010101010101010101"
        );

        let n1 = !n;
        assert_eq!(
            &n1.to_string(),
            "0xfefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefe"
        );

        assert_eq!(n & n1, I256::ZERO);
        assert_eq!(
            (n | n1).to_string().as_str(),
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        );
    }
}

// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};
use core::cmp::{Ord, Ordering, PartialOrd};
use core::fmt::{Display, Formatter};
use core::ops::{Add, BitAnd, BitOr, BitXor, Not, Sub};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use crate::encoding::hex::StartsWith0x;
use crate::math::Widening;
use crate::{cmp_elem, errors};

const N: usize = 3;
const MASK: u64 = 0xFFffFFff;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct U160 {
    n: [u64; N], // little endian
}

impl U160 {
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; 20] {
        let t: [u8; 24] = unsafe { core::mem::transmute_copy(&self.n) };
        let mut b = [0u8; 20];

        b.copy_from_slice(&t[..20]);
        b
    }

    #[inline]
    pub fn to_be_bytes(&self) -> [u8; 20] {
        let mut b = self.to_le_bytes();
        b.reverse();
        b
    }

    #[inline]
    pub fn from_le_bytes(buf: &[u8; 20]) -> Self {
        let mut t = [0u8; 24];
        t[..20].copy_from_slice(buf);
        Self { n: unsafe { core::mem::transmute_copy(&t) } }
    }

    #[inline]
    pub fn from_be_bytes(buf: &[u8; 20]) -> Self {
        let mut t = [0u8; 24];

        t[..20].copy_from_slice(buf);
        t.reverse();

        Self { n: unsafe { core::mem::transmute_copy(&t) } }
    }
}

impl Display for U160 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let h = self.to_be_bytes();

        f.write_str("0x")?;
        f.write_str(&hex::encode(&h))
    }
}

impl From<u64> for U160 {
    #[inline]
    fn from(value: u64) -> Self {
        Self { n: [value, 0, 0] }
    }
}

impl From<u128> for U160 {
    #[inline]
    fn from(value: u128) -> Self {
        Self { n: [value as u64, (value >> 64) as u64, 0] }
    }
}

impl PartialOrd for U160 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for U160 {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_elem!(self, other, 2);
        cmp_elem!(self, other, 1);
        cmp_elem!(self, other, 0);

        Ordering::Equal
    }
}

#[derive(Debug, Clone, Copy, errors::Error)]
pub enum ToU160Error {
    #[error("to-u160: hex-encode u160's length must be 40")]
    InvalidLength,

    #[error("to-u160: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for U160 {
    type Error = ToU160Error;

    /// value must be big-endian
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use hex::FromHexError as HexError;

        let value = if value.starts_with_0x() { &value[2..] } else { value };

        let mut buf = [0u8; 20];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|e| match e {
            HexError::OddLength | HexError::InvalidStringLength => Self::Error::InvalidLength,
            HexError::InvalidHexCharacter { c: ch, index: _ } => Self::Error::InvalidChar(ch),
        })?;

        Ok(Self::from_be_bytes(&buf))
    }
}

impl Serialize for U160 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for U160 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        U160::try_from(value.as_str()).map_err(D::Error::custom)
    }
}

impl Default for U160 {
    #[inline]
    fn default() -> Self {
        Self { n: [0; N] }
    }
}

impl Add for U160 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let (n0, carry0) = self.n[0].add_with_carrying(rhs.n[0], false);
        let (n1, carry1) = self.n[1].add_with_carrying(rhs.n[1], carry0);
        let (n2, _) = self.n[2].add_with_carrying(rhs.n[2], carry1);

        Self { n: [n0, n1, n2 & MASK] }
    }
}

impl Sub for U160 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let (n0, carry0) = self.n[0].sub_with_borrowing(rhs.n[0], false);
        let (n1, carry1) = self.n[1].sub_with_borrowing(rhs.n[1], carry0);
        let (n2, _) = self.n[2].sub_with_borrowing(rhs.n[2], carry1);

        Self { n: [n0, n1, n2 & MASK] }
    }
}

impl BitAnd for U160 {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] & rhs.n[0];
        let n1 = self.n[1] & rhs.n[1];
        let n2 = self.n[2] & rhs.n[2];

        Self { n: [n0, n1, n2 & MASK] }
    }
}

impl BitOr for U160 {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] | rhs.n[0];
        let n1 = self.n[1] | rhs.n[1];
        let n2 = self.n[2] | rhs.n[2];

        Self { n: [n0, n1, n2 & MASK] }
    }
}

impl BitXor for U160 {
    type Output = Self;

    #[inline]
    fn bitxor(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] ^ rhs.n[0];
        let n1 = self.n[1] ^ rhs.n[1];
        let n2 = self.n[2] ^ rhs.n[2];

        Self { n: [n0, n1, n2 & MASK] }
    }
}

impl Not for U160 {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        let n0 = !self.n[0];
        let n1 = !self.n[1];
        let n2 = !self.n[2];

        Self { n: [n0, n1, n2 & MASK] }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_uint160() {
        let u: U160 = u64::MAX.into();
        let v: U160 = u64::MAX.into();

        let w = u + v;
        let x: U160 = (u64::MAX as u128 + u64::MAX as u128).into();
        assert_eq!(w, x);

        let order = w.cmp(&x);
        assert_eq!(order, Ordering::Equal);

        let w: U160 = 1u64.into();
        let x: U160 = (1u128 << 64).into();
        let order = w.cmp(&x);
        assert_eq!(order, Ordering::Less);
        assert!(x > w);
    }
}

// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::{String, ToString};
use core::cmp::{Ord, PartialOrd, Ordering};
use core::fmt::{Display, Formatter};
use core::ops::{Add, AddAssign, Sub, SubAssign, BitAnd, BitOr, BitXor, Not};

use serde::{Serializer, Serialize, Deserializer, Deserialize, de::Error};

use crate::{errors, cmp_elem, math::Widening, encoding::{bin::*, hex::StartsWith0x}};


const N: usize = 4;


#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct U256 {
    n: [u64; N], // little endian
}

impl U256 {
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; 32] {
        // NOTE: assume platform endian is little endian
        unsafe { core::mem::transmute_copy(&self.n) }
    }

    #[inline]
    pub fn to_be_bytes(&self) -> [u8; 32] {
        let mut b = self.to_le_bytes();
        b.reverse();
        b
    }

    #[inline]
    pub fn from_le_bytes(buf: &[u8; 32]) -> Self {
        Self { n: unsafe { core::mem::transmute_copy(buf) } }
    }

    #[inline]
    pub fn from_be_bytes(buf: &[u8; 32]) -> Self {
        let mut buf = buf.clone();
        buf.reverse();
        Self::from_le_bytes(&buf)
    }

    #[inline]
    pub fn is_zero(&self) -> bool { self.eq(&Self::default()) }

    #[inline]
    pub fn is_even(&self) -> bool { self.n[0] & 1u64 == 0 }
}

impl Default for U256 {
    #[inline]
    fn default() -> Self { Self { n: [0; N] } }
}

impl Display for U256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let h = self.to_be_bytes();

        f.write_str("0x")?;
        f.write_str(&hex::encode(h))
    }
}


impl From<u64> for U256 {
    #[inline]
    fn from(value: u64) -> Self {
        Self { n: [value, 0, 0, 0] }
    }
}

impl From<u128> for U256 {
    #[inline]
    fn from(value: u128) -> Self {
        Self { n: [value as u64, (value >> 64) as u64, 0, 0] }
    }
}

impl PartialOrd for U256 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for U256 {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_elem!(self, other, 3);
        cmp_elem!(self, other, 2);
        cmp_elem!(self, other, 1);
        cmp_elem!(self, other, 0);

        Ordering::Equal
    }
}


#[derive(Debug, Clone, Copy, errors::Error)]
pub enum ToU256Error {
    #[error("to-u256: hex-encode u256's length must be 64")]
    InvalidLength,

    #[error("to-u256: invalid character {0}")]
    InvalidChar(char),
}

impl TryFrom<&str> for U256 {
    type Error = ToU256Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use hex::FromHexError as HexError;

        let value = if value.starts_with_0x() { &value[2..] } else { value };

        let mut buf = [0u8; 32];
        let _ = hex::decode_to_slice(value, &mut buf)
            .map_err(|e| match e {
                HexError::OddLength | HexError::InvalidStringLength => Self::Error::InvalidLength,
                HexError::InvalidHexCharacter { c: ch, index: _ } => Self::Error::InvalidChar(ch),
            })?;

        Ok(Self::from_be_bytes(&buf))
    }
}

impl Serialize for U256 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for U256 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        U256::try_from(value.as_str()).map_err(D::Error::custom)
    }
}

impl BinEncoder for U256 {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(self.to_le_bytes().as_slice());
    }

    fn bin_size(&self) -> usize { 32 }
}

impl BinDecoder for U256 {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut h = [0u8; 32];
        r.read_full(h.as_mut_slice())?;

        Ok(U256::from_le_bytes(&h))
    }
}

impl Add for U256 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let (n0, carry0) = self.n[0].add_with_carrying(rhs.n[0], false);
        let (n1, carry1) = self.n[1].add_with_carrying(rhs.n[1], carry0);
        let (n2, carry2) = self.n[2].add_with_carrying(rhs.n[2], carry1);
        let (n3, _) = self.n[3].add_with_carrying(rhs.n[3], carry2);

        Self { n: [n0, n1, n2, n3] }
    }
}

impl Add<u64> for U256 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: u64) -> Self::Output {
        self + U256::from(rhs)
    }
}

impl AddAssign for U256 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl AddAssign<u64> for U256 {
    #[inline]
    fn add_assign(&mut self, rhs: u64) {
        *self = *self + U256::from(rhs)
    }
}

impl Sub for U256 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let (n0, carry0) = self.n[0].sub_with_borrowing(rhs.n[0], false);
        let (n1, carry1) = self.n[1].sub_with_borrowing(rhs.n[1], carry0);
        let (n2, carry2) = self.n[2].sub_with_borrowing(rhs.n[2], carry1);
        let (n3, _) = self.n[3].sub_with_borrowing(rhs.n[3], carry2);

        Self { n: [n0, n1, n2, n3] }
    }
}

impl Sub<u64> for U256 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: u64) -> Self::Output {
        self - U256::from(rhs)
    }
}

impl SubAssign for U256 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl SubAssign<u64> for U256 {
    #[inline]
    fn sub_assign(&mut self, rhs: u64) {
        *self = *self - U256::from(rhs);
    }
}

impl BitAnd for U256 {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] & rhs.n[0];
        let n1 = self.n[1] & rhs.n[1];
        let n2 = self.n[2] & rhs.n[2];
        let n3 = self.n[3] & rhs.n[3];

        Self { n: [n0, n1, n2, n3] }
    }
}

impl BitOr for U256 {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] | rhs.n[0];
        let n1 = self.n[1] | rhs.n[1];
        let n2 = self.n[2] | rhs.n[2];
        let n3 = self.n[3] | rhs.n[3];

        Self { n: [n0, n1, n2, n3] }
    }
}

impl BitXor for U256 {
    type Output = Self;

    #[inline]
    fn bitxor(self, rhs: Self) -> Self::Output {
        let n0 = self.n[0] ^ rhs.n[0];
        let n1 = self.n[1] ^ rhs.n[1];
        let n2 = self.n[2] ^ rhs.n[2];
        let n3 = self.n[3] ^ rhs.n[3];

        Self { n: [n0, n1, n2, n3] }
    }
}

impl Not for U256 {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        let n0 = !self.n[0];
        let n1 = !self.n[1];
        let n2 = !self.n[2];
        let n3 = !self.n[3];

        Self { n: [n0, n1, n2, n3] }
    }
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;
    use crate::math::U256;

    #[test]
    fn test_uint256() {
        let u: U256 = u64::MAX.into();
        let v: U256 = u64::MAX.into();

        let w = u + v;
        let x: U256 = (u64::MAX as u128 + u64::MAX as u128).into();
        assert_eq!(w, x);

        let order = w.cmp(&x);
        assert_eq!(order, Ordering::Equal);

        let w: U256 = 1u64.into();
        let x: U256 = (1u128 << 64).into();
        let order = w.cmp(&x);
        assert_eq!(order, Ordering::Less);
        assert!(x > w);

        let z = w - U256::from(2u64);
        assert_eq!(z, !U256::default());

        let z = U256::from(1u64);
        let z = z - U256::from(2u64) + 3u64;
        assert_eq!(z, U256::from(2u64));
    }
}
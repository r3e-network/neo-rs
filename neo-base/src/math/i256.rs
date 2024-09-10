// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

// use alloc::string::{String, ToString};
// use core::cmp::{Ord, PartialOrd, Ordering};
use core::fmt::{Display, Formatter};

const N: usize = 4;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct I256 {
    n: [u64; N], // little endian
}

impl I256 {
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
        Self {
            n: unsafe { core::mem::transmute_copy(buf) },
        }
    }

    #[inline]
    pub fn from_be_bytes(buf: &[u8; 32]) -> Self {
        let mut buf = buf.clone();
        buf.reverse();
        Self::from_le_bytes(&buf)
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.eq(&Self::default())
    }

    #[inline]
    pub fn is_even(&self) -> bool {
        self.n[0] & 1u64 == 0
    }
}

impl Default for I256 {
    #[inline]
    fn default() -> Self {
        Self { n: [0; N] }
    }
}

impl Display for I256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let h = self.to_be_bytes();

        f.write_str("0x")?;
        f.write_str(&hex::encode(h))
    }
}

impl From<u8> for I256 {
    #[inline]
    fn from(value: u8) -> Self {
        Self {
            n: [value as u64, 0, 0, 0],
        }
    }
}

impl From<u16> for I256 {
    #[inline]
    fn from(value: u16) -> Self {
        Self {
            n: [value as u64, 0, 0, 0],
        }
    }
}

impl From<u32> for I256 {
    #[inline]
    fn from(value: u32) -> Self {
        Self {
            n: [value as u64, 0, 0, 0],
        }
    }
}

impl From<u64> for I256 {
    #[inline]
    fn from(value: u64) -> Self {
        Self {
            n: [value, 0, 0, 0],
        }
    }
}

impl From<u128> for I256 {
    #[inline]
    fn from(value: u128) -> Self {
        Self {
            n: [value as u64, (value >> 64) as u64, 0, 0],
        }
    }
}

impl From<i8> for I256 {
    #[inline]
    fn from(value: i8) -> Self {
        Self::from(value as i64)
    }
}

impl From<i16> for I256 {
    #[inline]
    fn from(value: i16) -> Self {
        Self::from(value as i64)
    }
}

impl From<i32> for I256 {
    #[inline]
    fn from(value: i32) -> Self {
        Self::from(value as i64)
    }
}

impl From<i64> for I256 {
    #[inline]
    fn from(value: i64) -> Self {
        let mut result = Self::default();
        result.n[0] = value as u64;
        if value < 0 {
            result.n[1] = u64::MAX;
            result.n[2] = u64::MAX;
            result.n[3] = u64::MAX;
        }
        result
    }
}

impl From<i128> for I256 {
    #[inline]
    fn from(value: i128) -> Self {
        let mut result = Self::default();
        result.n[0] = value as u64;
        result.n[1] = (value >> 64) as u64;
        if value < 0 {
            result.n[2] = u64::MAX;
            result.n[3] = u64::MAX;
        }
        result
    }
}

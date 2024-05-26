// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use core::cmp::{Ord, Ordering, PartialOrd};
use core::fmt::{Debug, Display, Formatter};

/// Int256 is a 256-bit integer, in little endian
#[derive(Copy, Clone, Default, Hash, Eq, PartialEq)]
#[repr(C)]
pub struct I256 {
    low: u128,
    high: i128,
}

#[cfg(target_endian = "little")]
impl I256 {
    pub const ZERO: Self = Self { low: 0, high: 0 };

    pub const ONE: Self = Self { low: 1, high: 0 };

    pub const MINUS_ONE: Self = Self {
        low: u128::MAX,
        high: -1,
    };

    pub const MAX: Self = Self {
        low: u128::MAX,
        high: i128::MAX,
    };

    pub const MIN: Self = Self {
        low: u128::MIN,
        high: i128::MIN,
    };

    #[inline]
    pub fn to_le_bytes(&self) -> [u8; 32] {
        unsafe { core::mem::transmute_copy(self) }
    }

    #[inline]
    pub fn to_be_bytes(&self) -> [u8; 32] {
        let mut bytes = self.to_le_bytes();
        bytes.reverse();
        bytes
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

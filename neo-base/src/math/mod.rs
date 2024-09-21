// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub use {i256::*, u160::*, u256::*};

pub mod i256;
pub mod u160;
pub mod u256;

pub trait Widening: Sized {
    const BITS: u8;

    type DoubleWidth;

    fn add_with_carrying(self, rhs: Self, carry: bool) -> (Self, bool);

    fn sub_with_borrowing(self, rhs: Self, carry: bool) -> (Self, bool);

    fn mul_with_carrying(self, rhs: Self, carry: Self) -> (Self, Self);

    fn mul_widening(self, rhs: Self) -> (Self, Self);
}

impl Widening for u64 {
    const BITS: u8 = 64;

    type DoubleWidth = u128;

    #[inline]
    fn add_with_carrying(self, rhs: Self, carry: bool) -> (Self, bool) {
        let (r1, o1) = self.overflowing_add(rhs);
        if carry {
            let (r2, o2) = r1.overflowing_add(1);
            (r2, o1 || o2)
        } else {
            (r1, o1)
        }
    }

    #[inline]
    fn sub_with_borrowing(self, rhs: Self, borrow: bool) -> (Self, bool) {
        let (r1, o1) = self.overflowing_sub(rhs);
        if borrow {
            let (s2, o2) = r1.overflowing_sub(1);
            (s2, o1 || o2)
        } else {
            (r1, o1)
        }
    }

    #[inline]
    fn mul_with_carrying(self, rhs: Self, carry: Self) -> (Self, Self) {
        let r = carry as Self::DoubleWidth + self as Self::DoubleWidth * rhs as Self::DoubleWidth;
        (r as Self, (r >> Self::BITS) as Self)
    }

    #[inline]
    fn mul_widening(self, rhs: Self) -> (Self, Self) {
        let r = self as Self::DoubleWidth * rhs as Self::DoubleWidth;
        (r as Self, (r >> Self::BITS) as Self)
    }
}

#[macro_export]
macro_rules! cmp_elem {
    ($lhs:ident, $rhs:ident, $n:expr) => {
        let order = $lhs.n[$n].cmp(&$rhs.n[$n]);
        if order != Ordering::Equal {
            return order;
        }
    };
}

/// Linear Congruential Generator
pub struct LcgRand {
    current: u64,
}

impl LcgRand {
    #[inline]
    pub fn new(seed: u64) -> LcgRand { LcgRand { current: seed } }

    #[inline]
    pub fn next(&mut self) -> u64 {
        self.current =
            self.current.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64);
        self.current
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_widening() {
        let u = 1u64;
        let (v, carry) = u.add_with_carrying(u, true);
        assert_eq!(v, 3);
        assert_eq!(carry, false);

        let (v, carry) = u.sub_with_borrowing(2, false);
        assert_eq!(v, u64::MAX);
        assert_eq!(carry, true);

        let (v, w) = u.mul_with_carrying(2, 1);
        assert_eq!(v, 3);
        assert_eq!(w, 0);

        let (v, w) = u64::MAX.mul_with_carrying(2, 0);
        assert_eq!(v, 0xfffffffffffffffe);
        assert_eq!(w, 1);
    }

    #[test]
    fn test_lcg_rand() {
        let mut lr = LcgRand::new(42);
        let n = lr.next();
        assert_eq!(n, 0x91778aed87ee5eb1);
    }
}

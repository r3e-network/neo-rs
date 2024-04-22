// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub mod uint256;
pub mod uint160;

pub use uint256::*;
pub use uint160::*;


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
        let r = carry as Self::DoubleWidth +
            self as Self::DoubleWidth * rhs as Self::DoubleWidth;
        (r as Self, (r >> Self::BITS) as Self)
    }

    #[inline]
    fn mul_widening(self, rhs: Self) -> (Self, Self) {
        let r = self as Self::DoubleWidth * rhs as Self::DoubleWidth;
        (r as Self, (r >> Self::BITS) as Self)
    }
}

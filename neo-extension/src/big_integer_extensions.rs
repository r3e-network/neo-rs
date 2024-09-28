use std::collections::HashSet;
use std::ops::{BitAnd, Shl};

use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};

pub trait BigIntegerExtensions {
    fn get_lowest_set_bit(&self) -> i32;
    fn mod_(&self, y: &BigInt) -> BigInt;
    fn mod_inverse(&self, n: &BigInt) -> Option<BigInt>;
    fn test_bit(&self, index: u32) -> bool;
}

impl BigIntegerExtensions for BigInt {
    fn get_lowest_set_bit(&self) -> i32 {
        if self.is_zero() {
            return -1;
        }
        let bytes = self.to_bytes_le().1;
        let mut w = 0;
        while w < bytes.len() && bytes[w] == 0 {
            w += 1;
        }
        if w == bytes.len() {
            return -1;
        }
        for x in 0..8 {
            if (bytes[w] & (1 << x)) > 0 {
                return (x + w * 8) as i32;
            }
        }
        unreachable!()
    }

    fn mod_(&self, y: &BigInt) -> BigInt {
        let mut x = self % y;
        if x.is_negative() {
            x += y;
        }
        x
    }

    fn mod_inverse(&self, n: &BigInt) -> Option<BigInt> {
        if self.gcd(n) != BigInt::one() {
            return None;
        }

        let (mut a, mut b) = (self.clone(), n.clone());
        let (mut x, mut y) = (BigInt::zero(), BigInt::one());

        while !a.is_zero() {
            let q = &b / &a;
            let r = &b % &a;

            let tmp = x - &q * &y;
            x = y;
            y = tmp;

            b = a;
            a = r;
        }

        if b != BigInt::one() {
            return None;
        }

        if x.is_negative() {
            x = x + n;
        }

        Some(x.mod_(n))
    }

    fn test_bit(&self, index: u32) -> bool {
        (self.bitand(BigInt::one().shl(index))) > BigInt::zero()
    }
}

pub trait BigIntegerIteratorExtensions: Iterator<Item = BigInt> {
    fn sum(self) -> BigInt;
}

impl<T: Iterator<Item = BigInt>> BigIntegerIteratorExtensions for T {
    fn sum(self) -> BigInt {
        self.fold(BigInt::zero(), |acc, x| acc + x)
    }
}

pub trait BigIntegerToByteArray {
    fn to_byte_array_standard(&self) -> Vec<u8>;
}

impl BigIntegerToByteArray for BigInt {
    fn to_byte_array_standard(&self) -> Vec<u8> {
        if self.is_zero() {
            return Vec::new();
        }
        self.to_bytes_le().1
    }
}

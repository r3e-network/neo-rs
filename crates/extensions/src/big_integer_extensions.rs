// Copyright (C) 2015-2025 The Neo Project.
//
// big_integer_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use num_bigint::BigInt;
use num_traits::identities::Zero;
use num_traits::sign::Signed;

/// BigInteger extensions matching C# BigIntegerExtensions exactly
pub trait BigIntegerExtensions {
    /// Finds the lowest set bit in the specified value. If value is zero, returns -1.
    /// Matches C# GetLowestSetBit method
    fn get_lowest_set_bit(&self) -> i32;

    /// Computes the remainder of the division of the specified value by the modulus.
    /// Matches C# Mod method
    fn mod_(&self, modulus: &BigInt) -> BigInt;

    /// Computes the modular inverse of the specified value.
    /// Matches C# ModInverse method
    fn mod_inverse(&self, modulus: &BigInt) -> Result<BigInt, String>;

    /// Tests whether the specified bit is set in the specified value.
    /// Matches C# TestBit method
    fn test_bit(&self, index: i32) -> bool;

    /// Finds the sum of the specified integers.
    /// Matches C# Sum method
    fn sum(&self) -> BigInt;

    /// Converts a BigInteger to byte array in little-endian and eliminates all the leading zeros.
    /// Matches C# ToByteArrayStandard method
    fn to_byte_array_standard(&self) -> Vec<u8>;

    /// Computes the square root of the specified value.
    /// Matches C# Sqrt method
    fn sqrt(&self) -> Result<BigInt, String>;

    /// Gets the number of bits required for shortest two's complement representation.
    /// Matches C# GetBitLength method
    fn get_bit_length(&self) -> i64;
}

impl BigIntegerExtensions for BigInt {
    fn get_lowest_set_bit(&self) -> i32 {
        if self.is_zero() {
            return -1;
        }

        let bytes = self.to_bytes_le().1;
        trailing_zero_count(&bytes)
    }

    fn mod_(&self, modulus: &BigInt) -> BigInt {
        let mut result = self % modulus;
        if result < BigInt::from(0) {
            result += modulus;
        }
        result
    }

    fn mod_inverse(&self, modulus: &BigInt) -> Result<BigInt, String> {
        if *self <= BigInt::from(0) {
            return Err("Value must be positive".to_string());
        }
        if *modulus < BigInt::from(2) {
            return Err("Modulus must be at least 2".to_string());
        }

        let mut r = self.clone();
        let mut old_r = modulus.clone();
        let mut s = BigInt::from(1);
        let mut old_s = BigInt::from(0);

        while r > BigInt::from(0) {
            let q = &old_r / &r;
            let temp_r = r.clone();
            r = &old_r % &r;
            old_r = temp_r;

            let temp_s = s.clone();
            s = &old_s - &q * &s;
            old_s = temp_s;
        }

        let mut result = &old_s % modulus;
        if result < BigInt::from(0) {
            result += modulus;
        }

        if (self * &result) % modulus != BigInt::from(1) {
            return Err("No modular inverse exists for the given inputs".to_string());
        }

        Ok(result)
    }

    fn test_bit(&self, index: i32) -> bool {
        if index < 0 {
            return false;
        }

        let bit_mask = BigInt::from(1) << index;
        !(self & &bit_mask).is_zero()
    }

    fn sum(&self) -> BigInt {
        self.clone()
    }

    fn to_byte_array_standard(&self) -> Vec<u8> {
        if self.is_zero() {
            return Vec::new();
        }
        self.to_bytes_le().1
    }

    fn sqrt(&self) -> Result<BigInt, String> {
        if *self < BigInt::from(0) {
            return Err(format!("value {} can not be negative for 'Sqrt'", self));
        }
        if self.is_zero() {
            return Ok(BigInt::from(0));
        }
        if *self < BigInt::from(4) {
            return Ok(BigInt::from(1));
        }

        let mut z = self.clone();
        let bit_length = (self.clone() - BigInt::from(1)).get_bit_length();
        let mut x = BigInt::from(1) << ((bit_length + 1) / 2);

        while x < z {
            z = x.clone();
            x = (self / &x + &x) / 2;
        }

        Ok(z)
    }

    fn get_bit_length(&self) -> i64 {
        if self.is_zero() || *self == BigInt::from(-1) {
            return 0;
        }

        let bytes = self.to_bytes_le().1;
        if bytes.len() == 1 || (bytes.len() == 2 && bytes[1] == 0) {
            return bit_count(if self.is_positive() {
                bytes[0]
            } else {
                255 - bytes[0]
            }) as i64;
        }

        let last_byte = if self.is_positive() {
            bytes[bytes.len() - 1]
        } else {
            255 - bytes[bytes.len() - 1]
        };
        (bytes.len() - 1) as i64 * 8 + bit_count(last_byte) as i64
    }
}

/// Extension for collections of BigInt
impl BigIntegerExtensions for Vec<BigInt> {
    fn get_lowest_set_bit(&self) -> i32 {
        -1 // Not applicable for collections
    }

    fn mod_(&self, _modulus: &BigInt) -> BigInt {
        BigInt::from(0) // Not applicable for collections
    }

    fn mod_inverse(&self, _modulus: &BigInt) -> Result<BigInt, String> {
        Err("Not applicable for collections".to_string())
    }

    fn test_bit(&self, _index: i32) -> bool {
        false // Not applicable for collections
    }

    fn sum(&self) -> BigInt {
        let mut sum = BigInt::from(0);
        for bi in self {
            sum += bi;
        }
        sum
    }

    fn to_byte_array_standard(&self) -> Vec<u8> {
        Vec::new() // Not applicable for collections
    }

    fn sqrt(&self) -> Result<BigInt, String> {
        Err("Not applicable for collections".to_string())
    }

    fn get_bit_length(&self) -> i64 {
        0 // Not applicable for collections
    }
}

/// Internal function to count trailing zeros
/// Matches C# TrailingZeroCount method
fn trailing_zero_count(bytes: &[u8]) -> i32 {
    let mut w = 0;
    while w < bytes.len() && bytes[w] == 0 {
        w += 1;
    }

    if w >= bytes.len() {
        return -1;
    }

    for x in 0..8 {
        if (bytes[w] & (1 << x)) > 0 {
            return (x + w * 8) as i32;
        }
    }

    -1
}

/// Internal function to get low part
/// Matches C# GetLowPart method
pub fn get_low_part(value: &BigInt, bit_count: i32) -> BigInt {
    let mask = (BigInt::from(1) << bit_count) - 1;
    value & mask
}

/// Internal function to count bits
/// Matches C# BitCount method
fn bit_count(w: u8) -> u8 {
    if w < 16 {
        if w < 4 {
            if w < 2 {
                w
            } else {
                2
            }
        } else if w < 8 {
            3
        } else {
            4
        }
    } else if w < 64 {
        if w < 32 {
            5
        } else {
            6
        }
    } else if w < 128 {
        7
    } else {
        8
    }
}

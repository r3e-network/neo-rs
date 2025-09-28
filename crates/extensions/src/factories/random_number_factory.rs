// Copyright (C) 2015-2025 The Neo Project.
//
// random_number_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use num_bigint::BigInt;
use rand::Rng;

/// Random number factory matching C# RandomNumberFactory exactly
pub struct RandomNumberFactory;

impl RandomNumberFactory {
    /// Generates a random sbyte.
    /// Matches C# NextSByte method
    pub fn next_sbyte() -> i8 {
        Self::next_sbyte_range(0, i8::MAX)
    }
    
    /// Generates a random sbyte with max value.
    /// Matches C# NextSByte method with maxValue
    pub fn next_sbyte_max(max_value: i8) -> Result<i8, String> {
        if max_value < 0 {
            return Err("maxValue must be non-negative".to_string());
        }
        Ok(Self::next_sbyte_range(0, max_value))
    }
    
    /// Generates a random sbyte in range.
    /// Matches C# NextSByte method with range
    pub fn next_sbyte_range(min_value: i8, max_value: i8) -> i8 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u32((max_value - min_value) as u32) as i8) + min_value
    }
    
    /// Generates a random byte.
    /// Matches C# NextByte method
    pub fn next_byte() -> u8 {
        Self::next_byte_range(0, u8::MAX)
    }
    
    /// Generates a random byte with max value.
    /// Matches C# NextByte method with maxValue
    pub fn next_byte_max(max_value: u8) -> u8 {
        Self::next_byte_range(0, max_value)
    }
    
    /// Generates a random byte in range.
    /// Matches C# NextByte method with range
    pub fn next_byte_range(min_value: u8, max_value: u8) -> u8 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u32((max_value - min_value) as u32) as u8) + min_value
    }
    
    /// Generates a random i16.
    /// Matches C# NextInt16 method
    pub fn next_i16() -> i16 {
        Self::next_i16_range(0, i16::MAX)
    }
    
    /// Generates a random i16 with max value.
    /// Matches C# NextInt16 method with maxValue
    pub fn next_i16_max(max_value: i16) -> Result<i16, String> {
        if max_value < 0 {
            return Err("maxValue must be non-negative".to_string());
        }
        Ok(Self::next_i16_range(0, max_value))
    }
    
    /// Generates a random i16 in range.
    /// Matches C# NextInt16 method with range
    pub fn next_i16_range(min_value: i16, max_value: i16) -> i16 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u32((max_value - min_value) as u32) as i16) + min_value
    }
    
    /// Generates a random u16.
    /// Matches C# NextUInt16 method
    pub fn next_u16() -> u16 {
        Self::next_u16_range(0, u16::MAX)
    }
    
    /// Generates a random u16 with max value.
    /// Matches C# NextUInt16 method with maxValue
    pub fn next_u16_max(max_value: u16) -> u16 {
        Self::next_u16_range(0, max_value)
    }
    
    /// Generates a random u16 in range.
    /// Matches C# NextUInt16 method with range
    pub fn next_u16_range(min_value: u16, max_value: u16) -> u16 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u32((max_value - min_value) as u32) as u16) + min_value
    }
    
    /// Generates a random i32.
    /// Matches C# NextInt32 method
    pub fn next_i32() -> i32 {
        Self::next_i32_range(0, i32::MAX)
    }
    
    /// Generates a random i32 with max value.
    /// Matches C# NextInt32 method with maxValue
    pub fn next_i32_max(max_value: i32) -> Result<i32, String> {
        if max_value < 0 {
            return Err("maxValue must be non-negative".to_string());
        }
        Ok(Self::next_i32_range(0, max_value))
    }
    
    /// Generates a random i32 in range.
    /// Matches C# NextInt32 method with range
    pub fn next_i32_range(min_value: i32, max_value: i32) -> i32 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u32((max_value - min_value) as u32) as i32) + min_value
    }
    
    /// Generates a random u32.
    /// Matches C# NextUInt32 method
    pub fn next_u32() -> u32 {
        let mut rng = rand::thread_rng();
        rng.gen()
    }
    
    /// Generates a random u32 with max value.
    /// Matches C# NextUInt32 method with maxValue
    pub fn next_u32_max(max_value: u32) -> u32 {
        if max_value == 0 {
            return 0;
        }
        
        let random_product = (max_value as u64) * Self::next_u32() as u64;
        let low_part = random_product as u32;
        
        if low_part < max_value {
            let remainder = (0u32.wrapping_sub(max_value)) % max_value;
            
            let mut current_low_part = low_part;
            while current_low_part < remainder {
                let new_random_product = (max_value as u64) * Self::next_u32() as u64;
                current_low_part = new_random_product as u32;
            }
        }
        
        (random_product >> 32) as u32
    }
    
    /// Generates a random u32 in range.
    /// Matches C# NextUInt32 method with range
    pub fn next_u32_range(min_value: u32, max_value: u32) -> u32 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        Self::next_u32_max(max_value - min_value) + min_value
    }
    
    /// Generates a random i64.
    /// Matches C# NextInt64 method
    pub fn next_i64() -> i64 {
        Self::next_i64_range(0, i64::MAX)
    }
    
    /// Generates a random i64 with max value.
    /// Matches C# NextInt64 method with maxValue
    pub fn next_i64_max(max_value: i64) -> i64 {
        Self::next_i64_range(0, max_value)
    }
    
    /// Generates a random i64 in range.
    /// Matches C# NextInt64 method with range
    pub fn next_i64_range(min_value: i64, max_value: i64) -> i64 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        (Self::next_u64((max_value - min_value) as u64) as i64) + min_value
    }
    
    /// Generates a random u64.
    /// Matches C# NextUInt64 method
    pub fn next_u64() -> u64 {
        let mut rng = rand::thread_rng();
        rng.gen()
    }
    
    /// Generates a random u64 with max value.
    /// Matches C# NextUInt64 method with maxValue
    pub fn next_u64_max(max_value: u64) -> u64 {
        let (random_product, low_part) = Self::big_mul(max_value, Self::next_u64());
        
        if low_part < max_value {
            let remainder = (0u64.wrapping_sub(max_value)) % max_value;
            
            let mut current_low_part = low_part;
            while current_low_part < remainder {
                let (new_random_product, new_low_part) = Self::big_mul(max_value, Self::next_u64());
                current_low_part = new_low_part;
            }
        }
        
        random_product
    }
    
    /// Generates a random u64 in range.
    /// Matches C# NextUInt64 method with range
    pub fn next_u64_range(min_value: u64, max_value: u64) -> u64 {
        if min_value == max_value {
            return max_value;
        }
        if min_value > max_value {
            panic!("minValue cannot be greater than maxValue");
        }
        Self::next_u64_max(max_value - min_value) + min_value
    }
    
    /// Generates a random BigInt in range.
    /// Matches C# NextBigInteger method with range
    pub fn next_big_int_range(min_value: &BigInt, max_value: &BigInt) -> Result<BigInt, String> {
        if min_value == max_value {
            return Ok(max_value.clone());
        }
        if min_value > max_value {
            return Err("minValue cannot be greater than maxValue".to_string());
        }
        Ok(Self::next_big_int_max(&(max_value - min_value))? + min_value)
    }
    
    /// Generates a random BigInt with max value.
    /// Matches C# NextBigInteger method with maxValue
    pub fn next_big_int_max(max_value: &BigInt) -> Result<BigInt, String> {
        if max_value < &BigInt::from(0) {
            return Err("maxValue must be non-negative".to_string());
        }
        
        let max_value_bits = max_value.bits() as usize;
        let max_value_size = BigInt::from(2).pow(max_value_bits as u32);
        
        let random_product = max_value * &Self::next_big_int_bits(max_value_bits)?;
        let random_product_bits = random_product.bits() as usize;
        
        let low_part = Self::get_low_part(&random_product, max_value_bits);
        
        if low_part < *max_value {
            let remainder = (&max_value_size - max_value) % max_value;
            
            let mut current_low_part = low_part.clone();
            while current_low_part < remainder {
                let new_random_product = max_value * &Self::next_big_int_bits(max_value_bits)?;
                current_low_part = Self::get_low_part(&new_random_product, max_value_bits);
            }
        }
        
        let result = &random_product >> (random_product_bits - max_value_bits);
        
        if result >= *max_value {
            Ok(BigInt::from(0))
        } else {
            Ok(result)
        }
    }
    
    /// Generates a random BigInt with specified bit size.
    /// Matches C# NextBigInteger method with sizeInBits
    pub fn next_big_int_bits(size_in_bits: usize) -> Result<BigInt, String> {
        if size_in_bits < 0 {
            return Err("sizeInBits must be non-negative".to_string());
        }
        if size_in_bits == 0 {
            return Ok(BigInt::from(0));
        }
        
        let mut rng = rand::thread_rng();
        let mut bytes = vec![0u8; size_in_bits / 8 + 1];
        rng.fill(&mut bytes[..]);
        
        if size_in_bits % 8 == 0 {
            bytes[bytes.len() - 1] = 0;
        } else {
            bytes[bytes.len() - 1] &= (1 << (size_in_bits % 8)) - 1;
        }
        
        Ok(BigInt::from_bytes_be(&bytes))
    }
    
    /// Big multiplication helper.
    /// Matches C# BigMul method
    fn big_mul(a: u64, b: u64) -> (u64, u64) {
        let al = a as u32;
        let ah = (a >> 32) as u32;
        let bl = b as u32;
        let bh = (b >> 32) as u32;
        
        let mull = (al as u64) * (bl as u64);
        let t = (ah as u64) * (bl as u64) + (mull >> 32);
        let tl = (al as u64) * (bh as u64) + (t as u32) as u64;
        
        let low = (tl << 32) | (mull as u32) as u64;
        let high = (ah as u64) * (bh as u64) + (t >> 32) + (tl >> 32);
        
        (high, low)
    }
    
    /// Gets the low part of a BigInt.
    /// Matches C# GetLowPart method
    fn get_low_part(value: &BigInt, bit_count: usize) -> BigInt {
        let mask = (BigInt::from(1) << bit_count) - 1;
        value & mask
    }
}

// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! VM integer stack item implementation for the Neo Virtual Machine.
//!
//! Provides the `VmInteger` enum that avoids heap allocation for values fitting in `i64`.

use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

/// VM integer that avoids heap allocation for values fitting in i64.
#[derive(Debug, Clone)]
pub enum VmInteger {
    Small(i64),
    Large(BigInt),
}

impl VmInteger {
    #[inline]
    pub fn from_bigint(value: BigInt) -> Self {
        match value.to_i64() {
            Some(small) => Self::Small(small),
            None => Self::Large(value),
        }
    }

    #[inline]
    pub fn to_bigint(&self) -> BigInt {
        match self {
            Self::Small(v) => BigInt::from(*v),
            Self::Large(v) => v.clone(),
        }
    }

    #[inline]
    pub fn into_bigint(self) -> BigInt {
        match self {
            Self::Small(v) => BigInt::from(v),
            Self::Large(v) => v,
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        match self {
            Self::Small(v) => *v == 0,
            Self::Large(v) => v.is_zero(),
        }
    }

    #[inline]
    pub fn is_one(&self) -> bool {
        match self {
            Self::Small(v) => *v == 1,
            Self::Large(v) => v == &BigInt::from(1),
        }
    }

    #[inline]
    pub fn is_positive(&self) -> bool {
        match self {
            Self::Small(v) => *v > 0,
            Self::Large(v) => v.sign() == num_bigint::Sign::Plus,
        }
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        match self {
            Self::Small(v) => *v < 0,
            Self::Large(v) => v.sign() == num_bigint::Sign::Minus,
        }
    }

    pub fn to_signed_bytes_le(&self) -> Vec<u8> {
        self.to_bigint().to_signed_bytes_le()
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Small(v) => Some(*v),
            Self::Large(v) => v.to_i64(),
        }
    }

    pub fn sign(&self) -> num_bigint::Sign {
        match self {
            Self::Small(v) if *v > 0 => num_bigint::Sign::Plus,
            Self::Small(v) if *v < 0 => num_bigint::Sign::Minus,
            Self::Small(_) => num_bigint::Sign::NoSign,
            Self::Large(v) => v.sign(),
        }
    }

    #[inline]
    pub fn vm_integer_stack_value(&self) -> StackValue {
        match self.to_i64() {
            Some(value) => StackValue::Integer(value),
            None => StackValue::BigInteger(self.to_signed_bytes_le()),
        }
    }
}

impl std::fmt::Display for VmInteger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small(v) => write!(f, "{v}"),
            Self::Large(v) => write!(f, "{v}"),
        }
    }
}

impl PartialEq for VmInteger {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Small(a), Self::Small(b)) => a == b,
            _ => self.to_bigint() == other.to_bigint(),
        }
    }
}
impl Eq for VmInteger {}

impl PartialOrd for VmInteger {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for VmInteger {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Small(a), Self::Small(b)) => a.cmp(b),
            _ => self.to_bigint().cmp(&other.to_bigint()),
        }
    }
}

impl PartialEq<BigInt> for VmInteger {
    fn eq(&self, other: &BigInt) -> bool {
        self.to_bigint() == *other
    }
}

// From impls so StackItem::Integer(x.into()) works for all integer types
impl From<BigInt> for VmInteger {
    fn from(v: BigInt) -> Self {
        Self::from_bigint(v)
    }
}
impl From<i64> for VmInteger {
    fn from(v: i64) -> Self {
        Self::Small(v)
    }
}
impl From<i32> for VmInteger {
    fn from(v: i32) -> Self {
        Self::Small(v as i64)
    }
}
impl From<u32> for VmInteger {
    fn from(v: u32) -> Self {
        Self::Small(v as i64)
    }
}
impl From<u64> for VmInteger {
    fn from(v: u64) -> Self {
        if let Ok(small) = i64::try_from(v) {
            Self::Small(small)
        } else {
            Self::Large(BigInt::from(v))
        }
    }
}
impl From<i128> for VmInteger {
    fn from(v: i128) -> Self {
        if let Ok(small) = i64::try_from(v) {
            Self::Small(small)
        } else {
            Self::Large(BigInt::from(v))
        }
    }
}

// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
extern crate core;

pub mod aes;
pub mod ecc256;
pub mod ecdsa;
pub mod hmac;
pub mod scrypt;

use subtle::ConstantTimeEq;
use zeroize::ZeroizeOnDrop;

/// SecretKey is an abstraction of a key that need to be kept secret
#[derive(Debug, Clone, ZeroizeOnDrop)]
pub struct SecretKey<const N: usize> {
    key: [u8; N],
}

impl<const N: usize> AsRef<[u8]> for SecretKey<N> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.key.as_ref()
    }
}

impl<const N: usize> AsRef<[u8; N]> for SecretKey<N> {
    #[inline]
    fn as_ref(&self) -> &[u8; N] {
        &self.key
    }
}

impl<const N: usize> Default for SecretKey<N> {
    #[inline]
    fn default() -> Self {
        Self { key: [0u8; N] }
    }
}

impl<const N: usize> Eq for SecretKey<N> {}

impl<const N: usize> PartialEq for SecretKey<N> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key.ct_eq(&other.key).into()
    }
}

impl<const N: usize> PartialEq<[u8]> for SecretKey<N> {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.key.ct_eq(other).into()
    }
}

// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::convert::{From, Into};

use subtle::ConstantTimeEq;
use zeroize::ZeroizeOnDrop;

use crate::rand::CryptoRand;

/// SecretKey is an abstraction of a key that need to be kept secret
#[derive(Debug, Clone, ZeroizeOnDrop)]
pub struct SecretKey<const N: usize> {
    key: [u8; N],
}

impl<const N: usize> SecretKey<N> {
    #[inline]
    pub fn from_crypto_rand<R: CryptoRand>(rand: &mut R) -> Result<Self, R::Error> {
        let mut key = SecretKey::<N>::default();
        rand.read_full(&mut key.key).map(|_| key)
    }

    #[inline]
    pub fn reverse(&self) -> Self {
        let mut key = self.clone();
        key.key.reverse();
        key
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.key.as_ref()
    }
}

impl<const N: usize> AsRef<[u8]> for SecretKey<N> {
    /// use it carefully
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.key.as_ref()
    }
}

impl<const N: usize> AsRef<[u8; N]> for SecretKey<N> {
    /// use it carefully
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

/// implement constant time equality for key
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

impl<const N: usize> Into<[u8; N]> for SecretKey<N> {
    #[inline]
    fn into(self) -> [u8; N] {
        self.key
    }
}

impl<const N: usize> From<[u8; N]> for SecretKey<N> {
    #[inline]
    fn from(key: [u8; N]) -> Self {
        Self { key }
    }
}

// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

//! High-level cryptography helpers used across the Neo N3 Rust stack.
//!
//! The crate intentionally wraps the lower-level primitives provided by
//! `p256`, `aes`, `hmac`, and `scrypt` so higher layers can remain protocol
//! focused.  All serialisation integrates with `neo-base`'s binary codec and
//! the design goals are documented in `docs/specs/neo-modules.md#neo-crypto`.

extern crate alloc;

pub mod aes;
pub mod bloom;
pub mod crypto;
pub mod ecc256;
pub mod ecdsa;
pub mod hash_algorithm;
pub mod hmac;
pub mod nep2;
pub mod scrypt;
pub mod secp256k1;

use alloc::fmt;
use subtle::ConstantTimeEq;
use zeroize::ZeroizeOnDrop;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

/// Heap allocated secret key wrapper that guarantees zeroisation on drop and
/// constant-time equality checks.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SecretKey<const N: usize> {
    key: [u8; N],
}

impl<const N: usize> SecretKey<N> {
    #[inline]
    pub fn from_array(array: [u8; N]) -> Self {
        Self { key: array }
    }

    #[inline]
    pub fn into_array(self) -> [u8; N] {
        self.key
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.key
    }
}

impl<const N: usize> Default for SecretKey<N> {
    #[inline]
    fn default() -> Self {
        Self { key: [0u8; N] }
    }
}

impl<const N: usize> fmt::Debug for SecretKey<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SecretKey").field(&"***").finish()
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

impl<const N: usize> AsRef<[u8]> for SecretKey<N> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<const N: usize> NeoEncode for SecretKey<N> {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.key);
    }
}

impl<const N: usize> NeoDecode for SecretKey<N> {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; N];
        reader.read_into(&mut buf)?;
        Ok(SecretKey { key: buf })
    }
}

pub use bloom::{BloomError, BloomFilter};
pub use crypto::{hash160, hash256, sign as sign_message, verify as verify_signature, Curve};
pub use ecc256::{Keypair, PrivateKey, PublicKey};
pub use ecdsa::{
    sign_with_algorithm, verify_with_algorithm, Secp256r1Sign, Secp256r1Verify, SignatureBytes,
};
pub use hash_algorithm::HashAlgorithm;
pub use nep2::{decrypt_nep2, encrypt_nep2, Nep2Error};
pub use secp256k1::{
    recover_public_key as secp256k1_recover_public_key,
    sign_recoverable as secp256k1_sign_recoverable, Secp256k1Error,
};

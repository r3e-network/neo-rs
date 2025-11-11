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
mod secret;

pub use secret::SecretKey;

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

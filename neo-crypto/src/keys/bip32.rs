//! BIP-32 helper primitives shared by wallet implementations.

use crate::{CryptoError, CryptoResult, ECCurve};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use num_traits::Zero;
use p256::elliptic_curve::{Curve, bigint::ArrayEncoding};
use sha2::Sha512;
use std::sync::LazyLock;

type HmacSha512 = Hmac<Sha512>;

static SECP256R1_ORDER: LazyLock<BigUint> =
    LazyLock::new(|| BigUint::from_bytes_be(&p256::NistP256::ORDER.to_be_byte_array()));

static SECP256K1_ORDER: LazyLock<BigUint> =
    LazyLock::new(|| BigUint::from_bytes_be(&k256::Secp256k1::ORDER.to_be_byte_array()));

/// Low-level BIP-32 cryptographic helpers.
///
/// This type intentionally does not parse derivation paths or own wallet state. It only exposes
/// primitives that depend on hashing and curve internals, keeping wallet orchestration outside the
/// crypto crate.
pub struct Bip32Crypto;

impl Bip32Crypto {
    /// Computes HMAC-SHA512 for BIP-32 key material.
    ///
    /// # Errors
    /// Returns an error if the HMAC key is rejected by the underlying implementation.
    pub fn hmac_sha512(key: &[u8], data: &[u8]) -> CryptoResult<[u8; 64]> {
        let mut mac = HmacSha512::new_from_slice(key)
            .map_err(|_| CryptoError::invalid_argument("Invalid HMAC key length"))?;
        mac.update(data);
        let result = mac.finalize().into_bytes();
        let mut out = [0u8; 64];
        out.copy_from_slice(&result);
        Ok(out)
    }

    /// Adds a BIP-32 child derivation factor to a parent private key modulo the curve order.
    ///
    /// # Errors
    /// Returns an error if `left_factor` is outside the curve order, if the derived key is zero,
    /// or if `curve` is not supported for BIP-32.
    pub fn add_private_keys_mod_order(
        left_factor: &[u8; 32],
        parent_private_key: &[u8; 32],
        curve: ECCurve,
    ) -> CryptoResult<[u8; 32]> {
        let order = curve_order(curve)?;
        add_mod_order(left_factor, parent_private_key, order)
    }
}

fn curve_order(curve: ECCurve) -> CryptoResult<&'static BigUint> {
    match curve {
        ECCurve::Secp256r1 => Ok(&SECP256R1_ORDER),
        ECCurve::Secp256k1 => Ok(&SECP256K1_ORDER),
        ECCurve::Ed25519 => Err(CryptoError::invalid_argument(
            "Ed25519 is not supported for BIP32",
        )),
    }
}

fn add_mod_order(a: &[u8], b: &[u8; 32], n: &BigUint) -> CryptoResult<[u8; 32]> {
    let a_int = BigUint::from_bytes_be(a);
    if a_int >= *n {
        return Err(CryptoError::invalid_argument(
            "Derived child private key is invalid.",
        ));
    }

    let b_int = BigUint::from_bytes_be(b);
    let r = (a_int + b_int) % n;
    if r.is_zero() {
        return Err(CryptoError::invalid_argument(
            "Derived child private key is invalid.",
        ));
    }

    let mut result = [0u8; 32];
    let r_bytes = r.to_bytes_be();
    result[32 - r_bytes.len()..].copy_from_slice(&r_bytes);
    Ok(result)
}

#[cfg(test)]
#[path = "../tests/keys/bip32.rs"]
mod tests;

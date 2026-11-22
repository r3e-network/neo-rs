//! Signature and key helpers used by Neo cryptographic APIs.

use crate::error::CryptoError;
use crate::{Crypto, CryptoResult, ECCurve, ECPoint, HashAlgorithm};
use core::convert::TryFrom;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use ed25519_dalek::{Signer as _, Verifier as _};
use k256::elliptic_curve::PrimeField as K256PrimeField;
use k256::elliptic_curve::group::prime::PrimeCurveAffine;
use k256::elliptic_curve::ops::MulByGenerator;
use k256::elliptic_curve::sec1::{
    FromEncodedPoint as K256FromEncodedPoint, ToEncodedPoint as K256ToEncodedPoint,
};
use k256::{
    AffinePoint as K256AffinePoint, EncodedPoint as K256EncodedPoint,
    ProjectivePoint as K256ProjectivePoint, Scalar as K256Scalar,
};
use num_bigint::{BigInt, Sign};
use num_traits::{One, Zero};
use p256::{
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
    ecdsa::{
        Signature, SigningKey, VerifyingKey,
        signature::hazmat::{PrehashSigner, PrehashVerifier},
    },
};
use rand::{RngCore, rngs::OsRng};
use secp256k1::{
    Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey,
    ecdsa::{RecoverableSignature, RecoveryId},
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// ECDSA operations for secp256k1 (Bitcoin's curve).
pub struct Secp256k1Crypto;

/// Maximum attempts for key generation to prevent infinite loops in case of RNG failure.
const MAX_KEY_GEN_ATTEMPTS: usize = 1000;

/// secp256k1 group order N (the C# `ECCurve.Secp256k1.BouncyCastleCurve.N` constant).
const SECP256K1_ORDER: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE,
    0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x41,
];

/// secp256k1 field prime P.
const SECP256K1_PRIME: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFC, 0x2F,
];

/// `(a mod m)` with a non-negative result, like BouncyCastle `BigInteger.Mod`.
fn mod_floor(value: &BigInt, modulus: &BigInt) -> BigInt {
    let rem = value % modulus;
    if rem.sign() == Sign::Minus {
        rem + modulus
    } else {
        rem
    }
}

/// Extended-Euclidean modular inverse; `None` when `value` and `modulus` are
/// not coprime (mirrors BouncyCastle `BigInteger.ModInverse`, which throws).
fn mod_inverse(value: &BigInt, modulus: &BigInt) -> Option<BigInt> {
    let (mut t, mut new_t) = (BigInt::zero(), BigInt::one());
    let (mut r, mut new_r) = (modulus.clone(), mod_floor(value, modulus));
    while new_r.sign() != Sign::NoSign {
        let quotient = &r / &new_r;
        let next_t = &t - &quotient * &new_t;
        t = std::mem::replace(&mut new_t, next_t);
        let next_r = &r - &quotient * &new_r;
        r = std::mem::replace(&mut new_r, next_r);
    }
    if r == BigInt::one() {
        Some(mod_floor(&t, modulus))
    } else {
        None
    }
}

/// Big-endian 32-byte encoding of a non-negative integer.
fn to_32_bytes_be(value: &BigInt) -> [u8; 32] {
    let bytes = value.to_bytes_be().1;
    let mut out = [0u8; 32];
    out[32 - bytes.len()..].copy_from_slice(&bytes);
    out
}

/// Parses the `(r, s, recId)` components exactly like the C# `Crypto
/// ECRecoverInternal`: 65-byte `r[32] || s[32] || v[1]` (v normalized from the
/// Ethereum 27..30 style) or 64-byte `r[32] || yParityAndS[32]` compact format
/// with the parity fused into the top bit of s.
fn parse_ec_recover_components(signature: &[u8]) -> CryptoResult<(BigInt, BigInt, u8)> {
    if signature.len() != 65 && signature.len() != 64 {
        return Err(CryptoError::invalid_signature(
            "Signature must be 65 or 64 bytes",
        ));
    }

    let r = BigInt::from_bytes_be(Sign::Plus, &signature[..32]);
    if signature.len() == 65 {
        // v could be 0..3 or 27..30 (Ethereum style).
        let v = signature[64];
        let rec_id = if v >= 27 { v - 27 } else { v };
        if rec_id > 3 {
            return Err(CryptoError::invalid_signature(
                "Recovery value must be in range [0..3] after normalization",
            ));
        }
        let s = BigInt::from_bytes_be(Sign::Plus, &signature[32..64]);
        Ok((r, s, rec_id))
    } else {
        // Mask out the top bit to get s; the bit itself is the yParity.
        let y_parity_and_s = BigInt::from_bytes_be(Sign::Plus, &signature[32..]);
        let mask = (BigInt::one() << 255u32) - 1u32;
        let s = &y_parity_and_s & &mask;
        let rec_id = u8::from(signature[32] & 0x80 != 0);
        Ok((r, s, rec_id))
    }
}

/// Rejects zero and order-size R/S values (the C# `checkRS` gate of
/// `Crypto.ECRecover`, active since Huyao).
fn check_rs(r: &BigInt, s: &BigInt) -> CryptoResult<()> {
    let order = BigInt::from_bytes_be(Sign::Plus, &SECP256K1_ORDER);
    if r.sign() == Sign::NoSign || s.sign() == Sign::NoSign || *r >= order || *s >= order {
        return Err(CryptoError::invalid_signature("Invalid R or S value"));
    }
    Ok(())
}

impl Secp256k1Crypto {
    /// Generates a new random private key.
    ///
    /// # Errors
    /// Returns an error if a valid key cannot be generated after `MAX_KEY_GEN_ATTEMPTS` attempts.
    /// This should only occur if the system RNG is misbehaving.
    pub fn generate_private_key() -> CryptoResult<[u8; 32]> {
        let mut rng = OsRng;
        for _ in 0..MAX_KEY_GEN_ATTEMPTS {
            let mut candidate = Zeroizing::new([0u8; 32]);
            rng.fill_bytes(candidate.as_mut());
            if let Ok(secret_key) = Secp256k1SecretKey::from_slice(candidate.as_ref()) {
                return Ok(secret_key.secret_bytes());
            }
        }
        Err(CryptoError::key_generation_failed(format!(
            "Failed to generate valid secp256k1 private key after {MAX_KEY_GEN_ATTEMPTS} attempts"
        )))
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 33]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let public_key = Secp256k1PublicKey::from_secret_key(&secp, &secret_key);
        Ok(public_key.serialize())
    }

    /// Signs a message with secp256k1.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature.serialize_compact())
    }

    /// Verifies a secp256k1 signature.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 33],
    ) -> CryptoResult<bool> {
        let secp = Secp256k1::verification_only();
        let public_key = Secp256k1PublicKey::from_slice(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp256k1::ecdsa::Signature::from_compact(signature)
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    /// Recovers a compressed secp256k1 public key from a message hash and signature.
    ///
    /// Accepts 65-byte (r||s||v) or 64-byte EIP-2098 compact signatures.
    ///
    /// This matches C# v3.11.2 `Crypto.ECRecover` (the Huyao
    /// `recoverSecp256K1`): zero and order-size R/S values are rejected before
    /// the recovery runs. The pre-Huyao compatibility behaviour lives in
    /// [`Secp256k1Crypto::recover_public_key_v0`].
    pub fn recover_public_key(message_hash: &[u8], signature: &[u8]) -> CryptoResult<Vec<u8>> {
        if message_hash.len() != 32 {
            return Err(CryptoError::invalid_argument(
                "Message hash must be 32 bytes",
            ));
        }

        let (r, s, rec_id) = parse_ec_recover_components(signature)?;
        check_rs(&r, &s)?;

        let msg = Message::from_digest_slice(message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message hash: {e}")))?;

        let mut sig_bytes = Vec::with_capacity(64);
        sig_bytes.extend_from_slice(&to_32_bytes_be(&r));
        sig_bytes.extend_from_slice(&to_32_bytes_be(&s));

        let rec_id = RecoveryId::from_i32(i32::from(rec_id))
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid recovery id: {e}")))?;
        let recoverable = RecoverableSignature::from_compact(&sig_bytes, rec_id).map_err(|e| {
            CryptoError::invalid_signature(format!("Invalid recoverable signature: {e}"))
        })?;

        let secp = Secp256k1::new();
        let public_key = secp
            .recover_ecdsa(&msg, &recoverable)
            .map_err(|e| CryptoError::invalid_key(format!("Failed to recover public key: {e}")))?;

        Ok(public_key.serialize().to_vec())
    }

    /// Recovers a compressed secp256k1 public key with the pre-Huyao
    /// compatibility behaviour (C# `Crypto.ECRecoverV0`, the BouncyCastle
    /// recovery that Neo exposes as `recoverSecp256K1` between Echidna and
    /// Huyao).
    ///
    /// R and S are NOT validated against the curve order: the BouncyCastle
    /// modular arithmetic wraps order-size components and treats a zero S as a
    /// zero scalar, so those inputs still produce a key here. The final
    /// compressed encoding mirrors the C# `point.EncodePoint(true)` result.
    pub fn recover_public_key_v0(message_hash: &[u8], signature: &[u8]) -> CryptoResult<Vec<u8>> {
        if message_hash.len() != 32 {
            return Err(CryptoError::invalid_argument(
                "Message hash must be 32 bytes",
            ));
        }

        let (r, s, rec_id) = parse_ec_recover_components(signature)?;

        let order = BigInt::from_bytes_be(Sign::Plus, &SECP256K1_ORDER);
        let prime = BigInt::from_bytes_be(Sign::Plus, &SECP256K1_PRIME);

        // Decompose recId into i = recId >> 1 and yBit = recId & 1.
        let i_part = u32::from(rec_id >> 1);
        let y_bit = rec_id & 1 == 1;

        let e = BigInt::from_bytes_be(Sign::Plus, message_hash);

        // eInv = -e mod n, rInv = (r^-1) mod n, srInv = (s * r^-1) mod n and
        // eInvrInv = (eInv * r^-1) mod n (C# ECRecoverInternal).
        let e_inv = mod_floor(&(-e), &order);
        let Some(r_inv) = mod_inverse(&r, &order) else {
            return Err(CryptoError::invalid_signature(
                "R is not invertible modulo the group order",
            ));
        };
        let sr_inv = mod_floor(&(&r_inv * &s), &order);
        let e_invr_inv = mod_floor(&(&r_inv * &e_inv), &order);

        // x = r + iPart * n, bounded by the field prime like the C# check.
        let x = &r + BigInt::from(i_part) * &order;
        if x >= prime {
            return Err(CryptoError::invalid_signature(
                "X coordinate is out of range for secp256k1 curve",
            ));
        }

        // Decompress R through SEC1 compressed decoding — mirrors the C#
        // `DecompressKey`/BouncyCastle `DecodePoint` call, failing when the
        // parity bit does not produce a curve point. The C# `R * n = infinity`
        // re-check is implied by the on-curve construction here.
        let mut compressed = [0u8; 33];
        compressed[0] = if y_bit { 0x03 } else { 0x02 };
        compressed[1..].copy_from_slice(&to_32_bytes_be(&x));
        let encoded = K256EncodedPoint::from_bytes(&compressed)
            .map_err(|_| CryptoError::invalid_signature("Invalid R point encoding"))?;
        let r_affine =
            Option::<K256AffinePoint>::from(K256AffinePoint::from_encoded_point(&encoded))
                .ok_or_else(|| {
                    CryptoError::invalid_signature("R point is not valid on the secp256k1 curve")
                })?;

        // Q = eInvrInv * G + srInv * R (C# SumOfTwoMultiplies).
        let e_invr_inv_scalar = K256Scalar::from_repr(to_32_bytes_be(&e_invr_inv).into());
        let sr_inv_scalar = K256Scalar::from_repr(to_32_bytes_be(&sr_inv).into());
        let (Some(e_invr_inv_scalar), Some(sr_inv_scalar)) = (
            Option::<K256Scalar>::from(e_invr_inv_scalar),
            Option::<K256Scalar>::from(sr_inv_scalar),
        ) else {
            return Err(CryptoError::invalid_signature(
                "Recovery scalars exceed the group order",
            ));
        };

        let q = K256ProjectivePoint::mul_by_generator(&e_invr_inv_scalar)
            + K256ProjectivePoint::from(r_affine) * sr_inv_scalar;
        let q_affine = q.to_affine();
        if bool::from(q_affine.is_identity()) {
            // C# cannot encode the point at infinity and surfaces it as an
            // ArgumentException (CryptoLib degrades that to null).
            return Err(CryptoError::invalid_signature(
                "Recovered point is the point at infinity",
            ));
        }

        Ok(q_affine.to_encoded_point(true).as_bytes().to_vec())
    }
}

/// ECDSA operations for secp256r1 (P-256, Neo's primary curve).
pub struct Secp256r1Crypto;

/// Signature prefix used by NeoFS ECDSA_SHA512 signatures.
pub const NEOFS_ECDSA_SHA512_PREFIX: u8 = 0x04;

/// Serialized NeoFS ECDSA_SHA512 signature length: one prefix byte plus raw P-256 ECDSA.
pub const NEOFS_ECDSA_SHA512_SIGNATURE_LEN: usize = 65;

impl Secp256r1Crypto {
    /// Generates a new random private key.
    pub fn generate_private_key() -> [u8; 32] {
        let secret_key = P256SecretKey::random(&mut OsRng);
        let bytes = secret_key.to_bytes();
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes.as_slice());
        key
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<Vec<u8>> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let verifying_key = VerifyingKey::from(&signing_key);
        Ok(verifying_key.to_encoded_point(true).as_bytes().to_vec())
    }

    /// Signs a message with secp256r1.
    ///
    /// The output is normalized to low-S: with RFC 6979 deterministic nonces a
    /// high-S result would be a fixed property of (key, message) instead of a
    /// coin flip, so it must be normalized to keep signatures canonical.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature: Signature = signing_key.sign(message);
        // normalize_s() returns the low-S variant (or None when already low-S).
        let signature = signature.normalize_s().unwrap_or(signature);
        let bytes: [u8; 64] = signature.to_bytes().into();
        Ok(bytes)
    }

    /// Signs a 32-byte message prehash with secp256r1.
    pub fn sign_prehash(
        message_digest: &[u8; 32],
        private_key: &[u8; 32],
    ) -> CryptoResult<[u8; 64]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature: Signature = signing_key
            .sign_prehash(message_digest)
            .map_err(|e| CryptoError::invalid_signature(format!("Failed to sign: {e}")))?;
        // normalize_s() returns the low-S variant (or None when already low-S).
        let signature = signature.normalize_s().unwrap_or(signature);
        Ok(signature.to_bytes().into())
    }

    /// Verifies a secp256r1 signature.
    ///
    /// `message` is hashed with SHA-256 internally (matches the p256 crate
    /// DigestVerifier trait). For Neo N3 protocol paths where the caller has
    /// already produced a 32-byte SHA-256 digest, prefer [`verify_prehash`]
    /// to avoid a second SHA-256 pass.
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8]) -> CryptoResult<bool> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);

        let signature = Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify(message, &signature).is_ok())
    }

    /// Verifies a secp256r1 signature over a 32-byte message prehash.
    ///
    /// This matches Neo N3 C# `ECDsa.VerifyHash` semantics: the caller is
    /// responsible for hashing the payload with SHA-256 before calling this
    /// function.  No additional hashing is performed internally.
    pub fn verify_prehash(
        message_digest: &[u8; 32],
        signature: &[u8; 64],
        public_key: &[u8],
    ) -> CryptoResult<bool> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);
        let signature = Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
        Ok(verifying_key
            .verify_prehash(message_digest, &signature)
            .is_ok())
    }

    /// Signs NeoFS data using P-256 over a SHA-512 prehash.
    ///
    /// NeoFS serializes this signature as `0x04 || raw_ecdsa_signature`.
    pub fn sign_neofs_sha512(
        data: &[u8],
        private_key: &[u8; 32],
    ) -> CryptoResult<[u8; NEOFS_ECDSA_SHA512_SIGNATURE_LEN]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let digest = Crypto::sha512(data);
        let signature: Signature = signing_key
            .sign_prehash(&digest)
            .map_err(|e| CryptoError::invalid_signature(format!("Failed to sign: {e}")))?;

        let mut output = [0u8; NEOFS_ECDSA_SHA512_SIGNATURE_LEN];
        output[0] = NEOFS_ECDSA_SHA512_PREFIX;
        output[1..].copy_from_slice(&signature.to_bytes());
        Ok(output)
    }

    /// Verifies a NeoFS P-256 signature over a SHA-512 prehash.
    ///
    /// The verifier preserves current NeoFS behavior by requiring a 65-byte signature while
    /// ignoring the first byte instead of enforcing the `0x04` prefix.
    pub fn verify_neofs_sha512(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> CryptoResult<bool> {
        if signature.len() != NEOFS_ECDSA_SHA512_SIGNATURE_LEN {
            return Err(CryptoError::invalid_signature(format!(
                "NeoFS signature must be {NEOFS_ECDSA_SHA512_SIGNATURE_LEN} bytes"
            )));
        }

        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);
        let signature = Signature::try_from(&signature[1..])
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
        let digest = Crypto::sha512(data);
        Ok(verifying_key.verify_prehash(&digest, &signature).is_ok())
    }
}

/// Ed25519 operations.
pub struct Ed25519Crypto;

impl Ed25519Crypto {
    /// Generates a new random private key using cryptographically secure RNG.
    pub fn generate_private_key() -> [u8; 32] {
        let signing_key = Ed25519SigningKey::generate(&mut OsRng);
        signing_key.to_bytes()
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 32]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        Ok(signing_key.verifying_key().to_bytes())
    }

    /// Signs a message with Ed25519.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Verifies an Ed25519 signature.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 32],
    ) -> CryptoResult<bool> {
        let verifying_key = Ed25519VerifyingKey::from_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let signature = Ed25519Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify_strict(message, &signature).is_ok())
    }
}

fn verify_ecdsa_raw64_with_hash(
    data: &[u8],
    signature: &[u8; 64],
    public_key: &[u8],
    curve: ECCurve,
    hash_algorithm: HashAlgorithm,
) -> CryptoResult<bool> {
    match (curve, hash_algorithm) {
        (ECCurve::Secp256k1, HashAlgorithm::Keccak256) => {
            let sig = secp256k1::ecdsa::Signature::from_compact(signature)
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let pubkey = Secp256k1PublicKey::from_slice(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let hash = Crypto::keccak256(data);
            let msg = Message::from_digest_slice(&hash)
                .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;
            Ok(Secp256k1::verification_only()
                .verify_ecdsa(&msg, &sig, &pubkey)
                .is_ok())
        }
        (ECCurve::Secp256r1, HashAlgorithm::Keccak256) => {
            let public_key = P256PublicKey::from_sec1_bytes(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let verifying_key = VerifyingKey::from(public_key);
            let signature = Signature::try_from(signature.as_slice())
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let hash = Crypto::keccak256(data);
            Ok(verifying_key.verify_prehash(&hash, &signature).is_ok())
        }
        (ECCurve::Secp256k1, _) => {
            let public_key: [u8; 33] = public_key
                .try_into()
                .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
            Secp256k1Crypto::verify(data, signature, &public_key)
        }
        (ECCurve::Secp256r1, _) => {
            let digest = Crypto::sha256(data);
            Secp256r1Crypto::verify_prehash(&digest, signature, public_key)
        }
        (ECCurve::Ed25519, _) => Err(CryptoError::invalid_argument(
            "Ed25519 is not an ECDSA curve",
        )),
    }
}

/// ECDSA operations wrapper.
pub struct ECDsa;

impl ECDsa {
    /// Signs data with ECDSA.
    pub fn sign(data: &[u8], private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<[u8; 64]> {
        match curve {
            ECCurve::Secp256k1 => Secp256k1Crypto::sign(data, private_key),
            ECCurve::Secp256r1 => {
                let digest = Crypto::sha256(data);
                Secp256r1Crypto::sign_prehash(&digest, private_key)
            }
            ECCurve::Ed25519 => Ed25519Crypto::sign(data, private_key),
        }
    }

    /// Verifies ECDSA signature.
    pub fn verify(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: ECCurve,
    ) -> CryptoResult<bool> {
        match curve {
            ECCurve::Secp256k1 => {
                if signature.len() != 64 || public_key.len() != 33 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256k1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Secp256r1 => {
                if signature.len() != 64 {
                    return Err(CryptoError::invalid_signature("Invalid signature length"));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256r1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Ed25519 => {
                if signature.len() != 64 || public_key.len() != 32 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                let pub_bytes: [u8; 32] = public_key
                    .try_into()
                    .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
                Ed25519Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
        }
    }
}

/// ECC operations wrapper.
pub struct ECC;

impl ECC {
    /// Generates a public key from private key.
    pub fn generate_public_key(private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<ECPoint> {
        match curve {
            ECCurve::Secp256k1 => {
                let pub_bytes = Secp256k1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Secp256r1 => {
                let pub_bytes = Secp256r1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Ed25519 => {
                let pub_bytes = Ed25519Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
        }
    }

    /// Compresses a public key.
    pub fn compress_public_key(public_key: &ECPoint) -> CryptoResult<Vec<u8>> {
        public_key
            .encode_compressed()
            .map_err(|e| CryptoError::invalid_point(e.to_string()))
    }
}

impl Crypto {
    /// Verifies ECDSA signature with secp256r1.
    #[must_use]
    pub fn verify_signature_secp256r1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256r1).unwrap_or(false)
    }

    /// Verifies ECDSA signature with secp256k1.
    #[must_use]
    pub fn verify_signature_secp256k1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256k1).unwrap_or(false)
    }

    /// Verifies an ECDSA signature using the specified curve and hash algorithm.
    #[must_use]
    pub fn verify_signature_with_curve(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: &ECCurve,
        hash_algorithm: HashAlgorithm,
    ) -> bool {
        if *curve == ECCurve::Ed25519 {
            return ECDsa::verify(data, signature, public_key, *curve).unwrap_or(false);
        }

        if signature.len() != 64 {
            return false;
        }

        let signature: [u8; 64] = match signature.try_into() {
            Ok(signature) => signature,
            Err(_) => return false,
        };

        verify_ecdsa_raw64_with_hash(data, &signature, public_key, *curve, hash_algorithm)
            .unwrap_or(false)
    }

    /// Verifies a signature against the supplied public key, inferring the curve where possible.
    #[must_use]
    pub fn verify_signature_bytes(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }

        let mut sig = [0u8; 64];
        sig.copy_from_slice(signature);

        match public_key.len() {
            32 => {
                let mut pk = [0u8; 32];
                pk.copy_from_slice(public_key);
                Ed25519Crypto::verify(message, &sig, &pk).unwrap_or(false)
            }
            33 => {
                // Neo N3 uses secp256r1 exclusively — try it first.
                let digest = Crypto::sha256(message);
                if Secp256r1Crypto::verify_prehash(&digest, &sig, public_key) == Ok(true) {
                    return true;
                }
                // secp256k1 fallback for non-Neo-N3 callers (placed after secp256r1).
                let mut pk = [0u8; 33];
                pk.copy_from_slice(public_key);
                Secp256k1Crypto::verify(message, &sig, &pk).unwrap_or(false)
            }
            64 | 65 => {
                let digest = Crypto::sha256(message);
                if Secp256r1Crypto::verify_prehash(&digest, &sig, public_key) == Ok(true) {
                    return true;
                }

                if let Ok(pk) = Secp256k1PublicKey::from_slice(public_key) {
                    let compressed = pk.serialize();
                    let mut buf = [0u8; 33];
                    buf.copy_from_slice(&compressed);
                    return Secp256k1Crypto::verify(message, &sig, &buf).unwrap_or(false);
                }
                false
            }
            _ => {
                let digest = Crypto::sha256(message);
                Secp256r1Crypto::verify_prehash(&digest, &sig, public_key).unwrap_or(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NEOFS_ECDSA_SHA512_PREFIX, SECP256K1_ORDER, Secp256k1Crypto, Secp256r1Crypto};
    use crate::{Crypto, ECCurve, HashAlgorithm};

    /// Half of the secp256r1 group order; an ECDSA signature is low-S when
    /// its s component (last 32 bytes, big-endian) is <= this value.
    const SECP256R1_HALF_ORDER: [u8; 32] = [
        0x7F, 0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xDE, 0x73, 0x7D, 0x56, 0xD3, 0x8B, 0xCF, 0x42, 0x79, 0xDC, 0xE5, 0x61, 0x7E, 0x31,
        0x92, 0xA8,
    ];

    fn is_low_s(signature: &[u8; 64]) -> bool {
        let s: &[u8] = &signature[32..];
        let half_order: &[u8] = &SECP256R1_HALF_ORDER;
        s <= half_order
    }

    #[test]
    fn secp256r1_signatures_are_low_s() {
        // RFC 6979 nonces are deterministic, so a high-S result would be a
        // fixed property of (key, message). Sample several pairs to make the
        // low-S guarantee explicit.
        for seed in 0u8..16 {
            let mut private_key = [0u8; 32];
            private_key[0] = seed.wrapping_mul(37).wrapping_add(1);
            private_key[31] = seed.wrapping_add(1);

            let message = format!("low-s sample {seed}");
            let signature = Secp256r1Crypto::sign(message.as_bytes(), &private_key).unwrap();
            assert!(
                is_low_s(&signature),
                "sign() produced high-S for seed {seed}"
            );

            let mut digest = Crypto::sha256(message.as_bytes());
            digest.reverse();
            let signature = Secp256r1Crypto::sign_prehash(&digest, &private_key).unwrap();
            assert!(
                is_low_s(&signature),
                "sign_prehash() produced high-S for seed {seed}"
            );
        }
    }

    #[test]
    fn test_secp256k1_operations() {
        let private_key = Secp256k1Crypto::generate_private_key().unwrap();
        let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"test message";

        let signature = Secp256k1Crypto::sign(message, &private_key).unwrap();
        let is_valid = Secp256k1Crypto::verify(message, &signature, &public_key).unwrap();

        assert!(is_valid);
    }

    #[test]
    fn secp256k1_recovery_matches_the_signer_key_on_both_versions() {
        for seed in 0u8..4 {
            let mut private_key = [0u8; 32];
            private_key[31] = seed.wrapping_add(11);
            let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
            let message = format!("recovery sample {seed}");
            let message_hash = Crypto::sha256(message.as_bytes());
            let signature = Secp256k1Crypto::sign(message.as_bytes(), &private_key).unwrap();

            // Huyao (V1) and pre-Huyao (V0) agree on well-formed signatures.
            // The 65-byte r||s||v form with v = 27/28 probes both R parities:
            // the two versions must recover identical keys and exactly one
            // parity hits the signer. (The raw 64-byte compact form cannot be
            // used directly: without the parity bit it only decodes to the
            // signer key for even recovery ids.)
            let mut sig65 = [0u8; 65];
            sig65[..64].copy_from_slice(&signature);
            let mut matched = false;
            for v in [27u8, 28] {
                sig65[64] = v;
                let recovered_v1 =
                    Secp256k1Crypto::recover_public_key(&message_hash, &sig65).unwrap();
                let recovered_v0 =
                    Secp256k1Crypto::recover_public_key_v0(&message_hash, &sig65).unwrap();
                assert_eq!(recovered_v1, recovered_v0, "seed {seed}");
                matched |= recovered_v1.as_slice() == public_key;
            }
            assert!(matched, "seed {seed}");

            // 64-byte compact form with the parity fused into the top bit of s:
            // exactly one of the two parities recovers the signer key.
            let mut compact = signature;
            let mut matched = false;
            for parity in [0u8, 1] {
                compact[32] = signature[32] | (parity << 7);
                let recovered_v1 =
                    Secp256k1Crypto::recover_public_key(&message_hash, &compact).unwrap();
                let recovered_v0 =
                    Secp256k1Crypto::recover_public_key_v0(&message_hash, &compact).unwrap();
                assert_eq!(recovered_v1, recovered_v0, "seed {seed}");
                matched |= recovered_v1.as_slice() == public_key;
            }
            assert!(matched, "seed {seed}");
        }
    }

    #[test]
    fn secp256k1_recovery_v1_rejects_invalid_r_s_and_v0_wraps_s() {
        let message = b"order-size probe";
        let mut private_key = [0u8; 32];
        private_key[31] = 23;
        let message_hash = Crypto::sha256(message);
        let signature = Secp256k1Crypto::sign(message, &private_key).unwrap();

        // 65-byte carrier so the raw s bytes reach the R/S validation.
        let mut sig65 = [0u8; 65];
        sig65[..64].copy_from_slice(&signature);
        sig65[64] = 27;

        // Huyao (V1) rejects zero and order-size R/S outright.
        let mut zero_s = sig65;
        zero_s[32..64].fill(0);
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &zero_s).is_err());

        let mut order_s = sig65;
        order_s[32..64].copy_from_slice(&SECP256K1_ORDER);
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &order_s).is_err());

        let mut order_r = sig65;
        order_r[..32].copy_from_slice(&SECP256K1_ORDER);
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &order_r).is_err());

        // Pre-Huyao (V0) wraps order-size S through the modular arithmetic:
        // s = N behaves exactly like s = 0 (srInv = 0) and still yields a key.
        let wrapped = Secp256k1Crypto::recover_public_key_v0(&message_hash, &order_s).unwrap();
        let zeroed = Secp256k1Crypto::recover_public_key_v0(&message_hash, &zero_s).unwrap();
        assert_eq!(wrapped, zeroed);
        assert_eq!(wrapped.len(), 33);
        assert!(wrapped[0] == 2 || wrapped[0] == 3);

        // ...while the order-size r stays unrecoverable (not invertible).
        assert!(Secp256k1Crypto::recover_public_key_v0(&message_hash, &order_r).is_err());
    }

    #[test]
    fn secp256k1_recovery_rejects_bad_lengths_and_recovery_ids() {
        let message_hash = Crypto::sha256(b"length probe");
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &[0u8; 63]).is_err());
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &[0u8; 66]).is_err());
        assert!(Secp256k1Crypto::recover_public_key(&[0u8; 31], &[0u8; 65]).is_err());
        assert!(Secp256k1Crypto::recover_public_key_v0(&message_hash, &[0u8; 63]).is_err());
        assert!(Secp256k1Crypto::recover_public_key_v0(&[0u8; 31], &[0u8; 65]).is_err());

        let mut sig = [0u8; 65];
        sig[64] = 31; // normalized recId 4
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &sig).is_err());
        assert!(Secp256k1Crypto::recover_public_key_v0(&message_hash, &sig).is_err());
        sig[64] = 26; // below the Ethereum range without wrapping to 0..3
        assert!(Secp256k1Crypto::recover_public_key(&message_hash, &sig).is_err());
        assert!(Secp256k1Crypto::recover_public_key_v0(&message_hash, &sig).is_err());
    }

    #[test]
    fn secp256r1_prehash_signs_keccak_digest() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"prehash payload";
        let digest = Crypto::keccak256(message);

        let signature = Secp256r1Crypto::sign_prehash(&digest, &private_key).unwrap();

        assert!(
            super::verify_ecdsa_raw64_with_hash(
                message,
                &signature,
                &public_key,
                ECCurve::Secp256r1,
                HashAlgorithm::Keccak256,
            )
            .unwrap()
        );
    }

    #[test]
    fn neofs_p256_sha512_signs_and_verifies() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";

        let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

        assert_eq!(signature.len(), 65);
        assert_eq!(signature[0], NEOFS_ECDSA_SHA512_PREFIX);
        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
    }

    #[test]
    fn neofs_p256_sha512_rejects_mutated_inputs() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

        assert!(
            !Secp256r1Crypto::verify_neofs_sha512(b"different message", &signature, &public_key)
                .unwrap()
        );

        let mut mutated = signature;
        mutated[64] ^= 0x01;
        assert!(!Secp256r1Crypto::verify_neofs_sha512(message, &mutated, &public_key).unwrap());

        assert!(
            Secp256r1Crypto::verify_neofs_sha512(message, &signature[..64], &public_key).is_err()
        );
        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &[0x02]).is_err());
    }

    #[test]
    fn neofs_p256_sha512_preserves_ignored_prefix_behavior() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let mut signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();
        signature[0] = 0xff;

        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
    }

    #[test]
    fn neofs_p256_sha512_rejects_regular_p256_signature() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let signature = Secp256r1Crypto::sign(message, &private_key).unwrap();
        let mut neofs_shaped_signature = [0u8; 65];
        neofs_shaped_signature[0] = NEOFS_ECDSA_SHA512_PREFIX;
        neofs_shaped_signature[1..].copy_from_slice(&signature);

        assert!(
            !Secp256r1Crypto::verify_neofs_sha512(message, &neofs_shaped_signature, &public_key)
                .unwrap()
        );
    }
}

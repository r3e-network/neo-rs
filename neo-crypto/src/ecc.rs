//! Elliptic Curve Cryptography for Neo blockchain.
//!
//! Supports secp256r1 (P-256/NIST), secp256k1, and Ed25519 curves.
//!
//! # Security
//! - All point construction validates that the point lies on the specified curve
//!   to prevent invalid-curve attacks.
//! - Key material uses constant-time comparisons to prevent timing side-channels.
//! - Sensitive data is zeroized on drop to prevent memory disclosure.

#![allow(unused_assignments)]

use crate::error::{CryptoError, CryptoResult};
use ed25519_dalek::{Signature as Ed25519Signature, SigningKey as Ed25519SigningKey, VerifyingKey};
use k256::{
    ecdsa::{
        Signature as K256Signature, SigningKey as K256SigningKey, VerifyingKey as K256VerifyingKey,
    },
    elliptic_curve::group::prime::PrimeCurveAffine,
    elliptic_curve::sec1::{
        FromEncodedPoint as K256FromEncodedPoint, ToEncodedPoint as K256ToEncodedPoint,
    },
    AffinePoint as K256AffinePoint, EncodedPoint as K256EncodedPoint,
};
use p256::{
    ecdsa::signature::Verifier,
    ecdsa::{
        Signature as P256Signature, SigningKey as P256SigningKey, VerifyingKey as P256VerifyingKey,
    },
    elliptic_curve::rand_core::OsRng,
    AffinePoint as P256AffinePoint, EncodedPoint as P256EncodedPoint,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Supported elliptic curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ECCurve {
    /// NIST P-256 curve (secp256r1) - Neo's primary curve
    Secp256r1,
    /// Bitcoin's curve (secp256k1)
    Secp256k1,
    /// Ed25519 curve for EdDSA
    Ed25519,
}

impl ECCurve {
    /// Returns the secp256r1 curve (Neo's default).
    pub fn secp256r1() -> Self {
        Self::Secp256r1
    }

    /// Returns the secp256k1 curve.
    pub fn secp256k1() -> Self {
        Self::Secp256k1
    }

    /// Returns the Ed25519 curve.
    pub fn ed25519() -> Self {
        Self::Ed25519
    }

    /// Returns the compressed public key size for this curve.
    pub fn compressed_size(&self) -> usize {
        match self {
            ECCurve::Secp256r1 | ECCurve::Secp256k1 => 33,
            ECCurve::Ed25519 => 32,
        }
    }

    /// Returns the uncompressed public key size for this curve.
    pub fn uncompressed_size(&self) -> usize {
        match self {
            ECCurve::Secp256r1 | ECCurve::Secp256k1 => 65,
            ECCurve::Ed25519 => 32, // Ed25519 doesn't have uncompressed format
        }
    }
}

/// Represents a point on an elliptic curve.
///
/// This is the primary type for public keys in Neo.
///
/// # Security
/// - Uses constant-time comparison to prevent timing side-channel attacks.
/// - Key material is automatically zeroized when the point is dropped.
#[allow(unused_assignments)]
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ECPoint {
    /// The curve this point belongs to.
    #[zeroize(skip)]
    #[allow(unused_assignments)]
    curve: ECCurve,
    /// Compressed representation of the point (33 bytes for secp256r1/k1, 32 for Ed25519).
    /// This field is zeroized on drop to prevent memory disclosure.
    data: Vec<u8>,
}

// Implement constant-time equality comparison to prevent timing attacks.
// This is critical for cryptographic operations where timing differences
// could leak information about secret keys.
impl ConstantTimeEq for ECPoint {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        // First check curve equality (not secret, can be variable-time)
        if self.curve != other.curve {
            return subtle::Choice::from(0);
        }
        // Constant-time comparison of the key data
        self.data.ct_eq(&other.data)
    }
}

impl PartialEq for ECPoint {
    fn eq(&self, other: &Self) -> bool {
        // Use constant-time comparison to prevent timing attacks
        self.ct_eq(other).into()
    }
}

impl Eq for ECPoint {}

// Hash/ordering are defined over the public compressed point bytes to mirror the
// C# implementation (which uses bytewise comparisons). These operations are
// variable-time but only ever apply to public key material, so they do not leak
// secrets. Equality checks for sensitive contexts must use `ct_eq` / `PartialEq`.
impl Hash for ECPoint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash is not timing-sensitive, use normal operations
        self.curve.hash(state);
        self.data.hash(state);
    }
}

impl ECPoint {
    /// Creates a new ECPoint from bytes with full on-curve validation.
    ///
    /// The input may be compressed (33 bytes for secp256r1/k1) or uncompressed (65 bytes).
    /// Points are stored internally in compressed form for consistency.
    pub fn new(curve: ECCurve, data: Vec<u8>) -> CryptoResult<Self> {
        Self::from_bytes_with_curve(curve, &data)
    }

    /// Parses a public key from bytes with explicit curve selection.
    ///
    /// Accepts compressed (33 bytes) or uncompressed (65 bytes) SEC1 encodings for
    /// secp256r1/k1, and 32-byte encodings for Ed25519. Points are validated to be on
    /// the specified curve and normalized to compressed form.
    pub fn from_bytes_with_curve(curve: ECCurve, data: &[u8]) -> CryptoResult<Self> {
        match curve {
            ECCurve::Secp256r1 => {
                let affine = Self::parse_p256_point(data)?;
                let compressed = affine.to_encoded_point(true);
                Self::new_unchecked(curve, compressed.as_bytes().to_vec())
            }
            ECCurve::Secp256k1 => {
                let affine = Self::parse_k256_point(data)?;
                let compressed = affine.to_encoded_point(true);
                Self::new_unchecked(curve, compressed.as_bytes().to_vec())
            }
            ECCurve::Ed25519 => {
                let bytes: [u8; 32] = data
                    .try_into()
                    .map_err(|_| CryptoError::invalid_point("Invalid Ed25519 point length"))?;

                // VerifyingKey::from_bytes validates that the point is on the curve
                VerifyingKey::from_bytes(&bytes).map_err(|e| {
                    CryptoError::invalid_point(format!("Invalid Ed25519 point: {}", e))
                })?;
                Self::new_unchecked(curve, bytes.to_vec())
            }
        }
    }

    /// Parses a public key from bytes, inferring the curve where possible.
    ///
    /// - 32 bytes: Ed25519
    /// - 33 or 65 bytes: tries secp256r1 first, then secp256k1
    pub fn from_bytes(data: &[u8]) -> CryptoResult<Self> {
        match data.len() {
            32 => Self::from_bytes_with_curve(ECCurve::Ed25519, data),
            33 | 65 => Self::from_bytes_with_curve(ECCurve::Secp256r1, data)
                .or_else(|_| Self::from_bytes_with_curve(ECCurve::Secp256k1, data)),
            _ => Err(CryptoError::invalid_point(format!(
                "Invalid point length: {}",
                data.len()
            ))),
        }
    }

    /// Creates a new ECPoint without on-curve validation.
    ///
    /// # Safety
    /// This method skips curve validation and should only be used when the point
    /// is known to be valid (e.g., from trusted internal sources or after prior validation).
    ///
    /// # Warning
    /// Using this with untrusted input can lead to invalid-curve attacks.
    pub fn new_unchecked(curve: ECCurve, data: Vec<u8>) -> CryptoResult<Self> {
        let expected_size = curve.compressed_size();
        if data.len() != expected_size {
            return Err(CryptoError::invalid_point(format!(
                "Invalid point size: expected {}, got {}",
                expected_size,
                data.len()
            )));
        }

        // Validate prefix for secp256r1/k1
        if matches!(curve, ECCurve::Secp256r1 | ECCurve::Secp256k1)
            && data[0] != 0x02
            && data[0] != 0x03
        {
            return Err(CryptoError::invalid_point(
                "Invalid compressed point prefix (expected 0x02 or 0x03)".to_string(),
            ));
        }

        Ok(Self { curve, data })
    }

    /// Creates an ECPoint from bytes with explicit curve specification.
    pub fn decode_compressed_with_curve(curve: ECCurve, data: &[u8]) -> CryptoResult<Self> {
        Self::from_bytes_with_curve(curve, data)
    }

    /// Creates an ECPoint from bytes, inferring the curve from data length.
    ///
    /// # Deprecated
    /// Use `decode_compressed_with_curve` for explicit curve specification.
    #[deprecated(
        since = "0.7.1",
        note = "Use decode_compressed_with_curve() with explicit curve to avoid curve confusion"
    )]
    pub fn decode_compressed(data: &[u8]) -> CryptoResult<Self> {
        Self::from_bytes(data)
    }

    /// Decodes a secp256r1 (P-256) point (compressed or uncompressed).
    pub fn decode_secp256r1(data: &[u8]) -> CryptoResult<Self> {
        Self::from_bytes_with_curve(ECCurve::Secp256r1, data)
    }

    /// Decodes a secp256k1 point (compressed or uncompressed).
    pub fn decode_secp256k1(data: &[u8]) -> CryptoResult<Self> {
        Self::from_bytes_with_curve(ECCurve::Secp256k1, data)
    }

    /// Decodes an Ed25519 public key.
    pub fn decode_ed25519(data: &[u8]) -> CryptoResult<Self> {
        Self::from_bytes_with_curve(ECCurve::Ed25519, data)
    }

    /// Returns the compressed representation of this point.
    pub fn encode_compressed(&self) -> CryptoResult<Vec<u8>> {
        Ok(self.data.clone())
    }

    /// Returns the curve this point belongs to.
    pub fn curve(&self) -> ECCurve {
        self.curve
    }

    /// Returns the raw bytes of this point.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Backward-compatible alias returning the compressed bytes as an owned `Vec<u8>`.
    ///
    /// Neo's Rust codebase historically used an `ECPoint` wrapper that exposed `to_bytes()`.
    /// The canonical representation in this crate is always compressed (except the internal
    /// infinity representation), so this returns the stored bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Backward-compatible alias returning the encoded bytes as an owned `Vec<u8>`.
    pub fn encoded(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Backward-compatible helper mirroring the legacy `is_compressed()` API.
    ///
    /// Note: this crate stores points in compressed form for secp256r1/k1 and as raw 32-byte
    /// keys for Ed25519.
    pub fn is_compressed(&self) -> bool {
        match self.curve {
            ECCurve::Secp256r1 | ECCurve::Secp256k1 => {
                self.data.len() == 33 && matches!(self.data.first(), Some(0x02) | Some(0x03))
            }
            ECCurve::Ed25519 => self.data.len() == 32,
        }
    }

    /// Backward-compatible helper mirroring the legacy `is_valid()` API.
    pub fn is_valid(&self) -> bool {
        self.is_on_curve()
    }

    /// Backward-compatible decoding helper mirroring `ECPoint::decode(data, curve)`.
    pub fn decode(data: &[u8], curve: ECCurve) -> Result<Self, String> {
        Self::from_bytes_with_curve(curve, data).map_err(|e| e.to_string())
    }

    /// Backward-compatible encoding helper mirroring `ECPoint::encode_point(compressed)`.
    ///
    /// - For secp256r1/k1, returns SEC1 compressed (33 bytes) or uncompressed (65 bytes).
    /// - For Ed25519, returns the 32-byte public key regardless of `compressed`.
    pub fn encode_point(&self, compressed: bool) -> Result<Vec<u8>, String> {
        if compressed {
            return self.encode_compressed().map_err(|e| e.to_string());
        }

        match self.curve {
            ECCurve::Secp256r1 => {
                let affine = Self::parse_p256_point(&self.data).map_err(|e| e.to_string())?;
                Ok(affine.to_encoded_point(false).as_bytes().to_vec())
            }
            ECCurve::Secp256k1 => {
                let affine = Self::parse_k256_point(&self.data).map_err(|e| e.to_string())?;
                Ok(affine.to_encoded_point(false).as_bytes().to_vec())
            }
            ECCurve::Ed25519 => Ok(self.data.clone()),
        }
    }

    /// Returns the infinity point (identity element) for the given curve.
    ///
    /// # Note
    /// The infinity point is represented using SEC1 encoding:
    /// - For secp256r1/secp256k1: single byte 0x00
    /// - For Ed25519: 32 zero bytes (Ed25519 identity point)
    ///
    /// This is a special internal representation and should not be serialized
    /// as a regular public key.
    pub fn infinity(curve: ECCurve) -> Self {
        let data = match curve {
            // SEC1 encoding for point at infinity is a single 0x00 byte
            ECCurve::Secp256r1 | ECCurve::Secp256k1 => vec![0x00],
            // Ed25519 identity point (all zeros)
            ECCurve::Ed25519 => vec![0u8; 32],
        };
        Self { curve, data }
    }

    /// Checks if this point is the infinity point (identity element).
    ///
    /// # Returns
    /// `true` if this is the point at infinity, `false` otherwise.
    pub fn is_infinity(&self) -> bool {
        match self.curve {
            ECCurve::Secp256r1 | ECCurve::Secp256k1 => {
                // SEC1 infinity is single 0x00 byte, or all zeros (legacy check)
                self.data.len() == 1 && self.data[0] == 0x00 || self.data.iter().all(|&b| b == 0)
            }
            ECCurve::Ed25519 => {
                // Ed25519 identity is all zeros
                self.data.iter().all(|&b| b == 0)
            }
        }
    }

    /// Returns true if the point is on the declared curve.
    pub fn is_on_curve(&self) -> bool {
        Self::validate_on_curve(self.curve, &self.data).is_ok()
    }

    /// Verifies a signature using this public key.
    ///
    /// For secp256r1/secp256k1, the message should be the message bytes (it will be hashed
    /// with SHA-256 by the underlying ECDSA implementation). For Ed25519, the message is
    /// verified directly.
    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> CryptoResult<bool> {
        verify_signature(self.curve, self.as_bytes(), message, signature)
    }

    /// Validates that the given point data represents a valid point on the specified curve.
    ///
    /// # Security
    /// This is critical for preventing invalid-curve attacks where an attacker provides
    /// a point that is not on the expected curve, potentially leaking private key bits.
    fn validate_on_curve(curve: ECCurve, data: &[u8]) -> CryptoResult<()> {
        match curve {
            ECCurve::Secp256r1 => {
                Self::parse_p256_point(data)?;
                Ok(())
            }
            ECCurve::Secp256k1 => {
                Self::parse_k256_point(data)?;
                Ok(())
            }
            ECCurve::Ed25519 => {
                let bytes: [u8; 32] = data.try_into().map_err(|_| {
                    CryptoError::invalid_point("Invalid Ed25519 point length".to_string())
                })?;

                VerifyingKey::from_bytes(&bytes).map_err(|e| {
                    CryptoError::invalid_point(format!("Invalid Ed25519 point: {}", e))
                })?;
                Ok(())
            }
        }
    }

    fn parse_p256_point(data: &[u8]) -> CryptoResult<P256AffinePoint> {
        if data.len() != ECCurve::Secp256r1.compressed_size()
            && data.len() != ECCurve::Secp256r1.uncompressed_size()
        {
            return Err(CryptoError::invalid_point(format!(
                "Invalid secp256r1 point size: expected {} or {}, got {}",
                ECCurve::Secp256r1.compressed_size(),
                ECCurve::Secp256r1.uncompressed_size(),
                data.len()
            )));
        }

        let encoded_point = P256EncodedPoint::from_bytes(data).map_err(|e| {
            CryptoError::invalid_point(format!("Invalid secp256r1 point encoding: {}", e))
        })?;

        let affine_point: Option<P256AffinePoint> =
            P256AffinePoint::from_encoded_point(&encoded_point).into();

        let Some(point) = affine_point else {
            return Err(CryptoError::invalid_point(
                "Point is not on the secp256r1 curve".to_string(),
            ));
        };

        if bool::from(point.is_identity()) {
            return Err(CryptoError::invalid_point(
                "Point at infinity is not a valid secp256r1 public key".to_string(),
            ));
        }

        Ok(point)
    }

    fn parse_k256_point(data: &[u8]) -> CryptoResult<K256AffinePoint> {
        if data.len() != ECCurve::Secp256k1.compressed_size()
            && data.len() != ECCurve::Secp256k1.uncompressed_size()
        {
            return Err(CryptoError::invalid_point(format!(
                "Invalid secp256k1 point size: expected {} or {}, got {}",
                ECCurve::Secp256k1.compressed_size(),
                ECCurve::Secp256k1.uncompressed_size(),
                data.len()
            )));
        }

        let encoded_point = K256EncodedPoint::from_bytes(data).map_err(|e| {
            CryptoError::invalid_point(format!("Invalid secp256k1 point encoding: {}", e))
        })?;

        let affine_point: Option<K256AffinePoint> =
            K256AffinePoint::from_encoded_point(&encoded_point).into();

        let Some(point) = affine_point else {
            return Err(CryptoError::invalid_point(
                "Point is not on the secp256k1 curve".to_string(),
            ));
        };

        if bool::from(point.is_identity()) {
            return Err(CryptoError::invalid_point(
                "Point at infinity is not a valid secp256k1 public key".to_string(),
            ));
        }

        Ok(point)
    }
}

impl fmt::Debug for ECPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ECPoint({:?}, {})", self.curve, hex::encode(&self.data))
    }
}

impl fmt::Display for ECPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.data))
    }
}

impl PartialOrd for ECPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Ordering mirrors the C# `Neo.Cryptography.ECC.ECPoint.CompareTo` implementation:
// - Points must be on the same curve to be comparable in C# (it throws otherwise).
// - Infinity compares before finite points.
// - Finite points compare by X coordinate, then Y coordinate (numeric compare).
//
// For robustness/determinism in Rust, we define an order across curves by comparing
// the curve first, then applying the C# ordering within the curve.
impl Ord for ECPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.curve.cmp(&other.curve) {
            Ordering::Equal => {}
            non_equal => return non_equal,
        }

        match (self.is_infinity(), other.is_infinity()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }

        match self.curve {
            ECCurve::Secp256r1 => {
                let left = Self::parse_p256_point(&self.data)
                    .expect("validated secp256r1 points must remain parseable");
                let right = Self::parse_p256_point(&other.data)
                    .expect("validated secp256r1 points must remain parseable");

                let left = left.to_encoded_point(false);
                let right = right.to_encoded_point(false);

                let left_x = left.x().expect("uncompressed point must have X");
                let right_x = right.x().expect("uncompressed point must have X");
                match left_x.cmp(right_x) {
                    Ordering::Equal => {}
                    non_equal => return non_equal,
                }

                let left_y = left.y().expect("uncompressed point must have Y");
                let right_y = right.y().expect("uncompressed point must have Y");
                left_y.cmp(right_y)
            }
            ECCurve::Secp256k1 => {
                let left = Self::parse_k256_point(&self.data)
                    .expect("validated secp256k1 points must remain parseable");
                let right = Self::parse_k256_point(&other.data)
                    .expect("validated secp256k1 points must remain parseable");

                let left = left.to_encoded_point(false);
                let right = right.to_encoded_point(false);

                let left_x = left.x().expect("uncompressed point must have X");
                let right_x = right.x().expect("uncompressed point must have X");
                match left_x.cmp(right_x) {
                    Ordering::Equal => {}
                    non_equal => return non_equal,
                }

                let left_y = left.y().expect("uncompressed point must have Y");
                let right_y = right.y().expect("uncompressed point must have Y");
                left_y.cmp(right_y)
            }
            // Ed25519 isn't used by Neo N3 consensus/committee keys, but keep a deterministic order.
            ECCurve::Ed25519 => self.data.cmp(&other.data),
        }
    }
}

impl PartialOrd for ECCurve {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ECCurve {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// Verifies a signature for the specified curve.
///
/// For secp256r1/secp256k1 the message is hashed internally using SHA-256 by the
/// ECDSA implementation. For Ed25519, the message is verified directly.
pub fn verify_signature(
    curve: ECCurve,
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> CryptoResult<bool> {
    match curve {
        ECCurve::Secp256r1 => verify_signature_secp256r1(public_key, message, signature),
        ECCurve::Secp256k1 => verify_signature_secp256k1(public_key, message, signature),
        ECCurve::Ed25519 => verify_ed25519(public_key, message, signature),
    }
}

/// Verifies a secp256r1 (P-256) signature.
pub fn verify_signature_secp256r1(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> CryptoResult<bool> {
    let verifying_key = P256VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|e| CryptoError::invalid_key(format!("Invalid secp256r1 public key: {}", e)))?;

    let sig = P256Signature::from_der(signature)
        .or_else(|_| P256Signature::from_slice(signature))
        .map_err(|e| {
            CryptoError::invalid_signature(format!("Invalid secp256r1 signature: {}", e))
        })?;

    Ok(verifying_key.verify(message, &sig).is_ok())
}

/// Verifies a secp256k1 signature.
pub fn verify_signature_secp256k1(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> CryptoResult<bool> {
    let verifying_key = K256VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|e| CryptoError::invalid_key(format!("Invalid secp256k1 public key: {}", e)))?;

    let sig = K256Signature::from_der(signature)
        .or_else(|_| K256Signature::from_slice(signature))
        .map_err(|e| {
            CryptoError::invalid_signature(format!("Invalid secp256k1 signature: {}", e))
        })?;

    Ok(verifying_key.verify(message, &sig).is_ok())
}

/// Verifies an Ed25519 signature.
pub fn verify_ed25519(public_key: &[u8], message: &[u8], signature: &[u8]) -> CryptoResult<bool> {
    let pk_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| CryptoError::invalid_key("Ed25519 public key must be 32 bytes"))?;
    let sig = Ed25519Signature::try_from(signature)
        .map_err(|e| CryptoError::invalid_signature(format!("Invalid Ed25519 signature: {}", e)))?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| CryptoError::invalid_key(format!("Invalid Ed25519 public key: {}", e)))?;

    Ok(verifying_key.verify_strict(message, &sig).is_ok())
}

/// Generates a random keypair for the specified curve.
pub fn generate_keypair(curve: ECCurve) -> CryptoResult<(Vec<u8>, ECPoint)> {
    match curve {
        ECCurve::Secp256r1 => {
            let signing_key = P256SigningKey::random(&mut OsRng);
            let verifying_key = signing_key.verifying_key();
            let private_key = signing_key.to_bytes().to_vec();
            let public_point = ECPoint::new_unchecked(
                curve,
                verifying_key.to_encoded_point(true).as_bytes().to_vec(),
            )?;
            Ok((private_key, public_point))
        }
        ECCurve::Secp256k1 => {
            let signing_key = K256SigningKey::random(&mut OsRng);
            let verifying_key = signing_key.verifying_key();
            let private_key = signing_key.to_bytes().to_vec();
            let public_point = ECPoint::new_unchecked(
                curve,
                verifying_key.to_encoded_point(true).as_bytes().to_vec(),
            )?;
            Ok((private_key, public_point))
        }
        ECCurve::Ed25519 => {
            let signing_key = Ed25519SigningKey::generate(&mut OsRng);
            let private_key = signing_key.to_bytes().to_vec();
            let public_point =
                ECPoint::new_unchecked(curve, signing_key.verifying_key().to_bytes().to_vec())?;
            Ok((private_key, public_point))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer as _;

    fn scalar(value: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[31] = value;
        bytes
    }

    fn p256_signing_key() -> P256SigningKey {
        P256SigningKey::from_bytes(&p256::FieldBytes::from(scalar(1))).unwrap()
    }

    fn k256_signing_key() -> K256SigningKey {
        K256SigningKey::from_bytes(&k256::FieldBytes::from(scalar(2))).unwrap()
    }

    fn ed25519_signing_key() -> Ed25519SigningKey {
        Ed25519SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn test_ec_curve_sizes() {
        assert_eq!(ECCurve::Secp256r1.compressed_size(), 33);
        assert_eq!(ECCurve::Secp256k1.compressed_size(), 33);
        assert_eq!(ECCurve::Ed25519.compressed_size(), 32);

        assert_eq!(ECCurve::Secp256r1.uncompressed_size(), 65);
        assert_eq!(ECCurve::Secp256k1.uncompressed_size(), 65);
        assert_eq!(ECCurve::Ed25519.uncompressed_size(), 32);
    }

    #[test]
    fn test_ec_point_creation() {
        let signing_key = p256_signing_key();
        let pub_bytes = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let point = ECPoint::new(ECCurve::Secp256r1, pub_bytes.clone()).unwrap();
        assert_eq!(point.curve(), ECCurve::Secp256r1);
        assert_eq!(point.as_bytes(), pub_bytes.as_slice());
        assert!(point.is_on_curve());
    }

    #[test]
    fn test_ec_point_invalid_prefix() {
        let mut data = vec![0x04]; // Invalid prefix for compressed
        data.extend_from_slice(&[0xAA; 32]);
        let result = ECPoint::new(ECCurve::Secp256r1, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_ec_point_invalid_size() {
        let data = vec![0x02; 20]; // Wrong size
        let result = ECPoint::new(ECCurve::Secp256r1, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_ec_point_infinity() {
        let infinity = ECPoint::infinity(ECCurve::Secp256r1);
        assert!(infinity.is_infinity());
        assert!(!infinity.is_on_curve());
    }

    #[test]
    fn test_ec_point_decode_compressed() {
        let signing_key = p256_signing_key();
        let compressed = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let point = ECPoint::from_bytes(&compressed).unwrap();
        assert_eq!(point.curve(), ECCurve::Secp256r1);
        assert!(point.is_on_curve());
    }

    #[test]
    fn test_ec_point_from_uncompressed() {
        let signing_key = p256_signing_key();
        let uncompressed = signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let point = ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, &uncompressed).unwrap();
        assert_eq!(point.as_bytes().len(), ECCurve::Secp256r1.compressed_size());
        assert!(point.is_on_curve());
    }

    #[test]
    fn test_ec_point_ordering() {
        let first = p256_signing_key();
        let second = P256SigningKey::from_bytes(&p256::FieldBytes::from(scalar(3))).unwrap();

        let point1 = ECPoint::new(
            ECCurve::Secp256r1,
            first
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
        let point2 = ECPoint::new(
            ECCurve::Secp256r1,
            second
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .to_vec(),
        )
        .unwrap();

        assert_ne!(point1, point2);
        assert_eq!(
            point1.cmp(&point2),
            point1.as_bytes().cmp(point2.as_bytes())
        );
    }

    #[test]
    fn test_verify_secp256r1_signature() {
        let signing_key = p256_signing_key();
        let pub_bytes = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let message = b"neo-secp256r1";
        let signature: P256Signature = signing_key.sign(message);
        let signature_bytes = signature.to_bytes();

        assert!(
            verify_signature_secp256r1(&pub_bytes, message, signature_bytes.as_slice()).unwrap()
        );

        let mut bad_sig = signature_bytes;
        bad_sig[0] ^= 0x01;
        assert!(!verify_signature_secp256r1(&pub_bytes, message, &bad_sig).unwrap());
    }

    #[test]
    fn test_verify_secp256k1_signature() {
        let signing_key = k256_signing_key();
        let pub_bytes = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let message = b"neo-secp256k1";
        let signature: K256Signature = signing_key.sign(message);
        let signature_bytes = signature.to_bytes();

        assert!(
            verify_signature_secp256k1(&pub_bytes, message, signature_bytes.as_slice()).unwrap()
        );

        let mut bad_sig = signature_bytes;
        bad_sig[0] ^= 0x01;
        assert!(!verify_signature_secp256k1(&pub_bytes, message, &bad_sig).unwrap());
    }

    #[test]
    fn test_verify_ed25519_signature() {
        let signing_key = ed25519_signing_key();
        let verifying_key = signing_key.verifying_key();
        let message = b"neo-ed25519";
        let signature = signing_key.sign(message);

        assert!(verify_ed25519(&verifying_key.to_bytes(), message, &signature.to_bytes()).unwrap());

        let mut bad_sig = signature.to_bytes();
        bad_sig[0] ^= 0x01;
        assert!(!verify_ed25519(&verifying_key.to_bytes(), message, &bad_sig).unwrap());
    }

    #[test]
    fn test_generate_keypair_roundtrip() {
        let (private_key, public_point) = generate_keypair(ECCurve::Secp256r1).unwrap();
        assert_eq!(
            public_point.as_bytes().len(),
            ECCurve::Secp256r1.compressed_size()
        );

        let private_array: [u8; 32] = private_key.try_into().unwrap();
        let signing_key =
            P256SigningKey::from_bytes(&p256::FieldBytes::from(private_array)).unwrap();
        let message = b"keygen-roundtrip";
        let signature: P256Signature = signing_key.sign(message);
        let signature_bytes = signature.to_bytes();

        assert!(verify_signature(
            ECCurve::Secp256r1,
            public_point.as_bytes(),
            message,
            signature_bytes.as_slice()
        )
        .unwrap());
    }
}

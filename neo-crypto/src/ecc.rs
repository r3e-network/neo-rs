//! Elliptic Curve Cryptography for Neo blockchain.
//!
//! Supports secp256r1 (P-256/NIST), secp256k1, and Ed25519 curves.
//!
//! # Security
//! - All point construction validates that the point lies on the specified curve
//!   to prevent invalid-curve attacks.
//! - Key material uses constant-time comparisons to prevent timing side-channels.
//! - Sensitive data is zeroized on drop to prevent memory disclosure.

use crate::error::{CryptoError, CryptoResult};
use ed25519_dalek::VerifyingKey;
use k256::elliptic_curve::sec1::FromEncodedPoint as K256FromEncodedPoint;
#[allow(unused_imports)]
use p256::elliptic_curve::sec1::FromEncodedPoint as P256FromEncodedPoint;
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
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ECPoint {
    /// The curve this point belongs to.
    #[zeroize(skip)]
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

impl Hash for ECPoint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash is not timing-sensitive, use normal operations
        self.curve.hash(state);
        self.data.hash(state);
    }
}

impl ECPoint {
    /// Creates a new ECPoint from compressed bytes with full on-curve validation.
    ///
    /// # Arguments
    /// * `curve` - The elliptic curve
    /// * `data` - Compressed point data
    ///
    /// # Returns
    /// A new ECPoint or an error if the data is invalid or the point is not on the curve.
    ///
    /// # Security
    /// This method validates that the point lies on the specified curve to prevent
    /// invalid-curve attacks. Invalid or low-order points are rejected.
    pub fn new(curve: ECCurve, data: Vec<u8>) -> CryptoResult<Self> {
        let expected_size = curve.compressed_size();
        if data.len() != expected_size {
            return Err(CryptoError::invalid_point(format!(
                "Invalid point size: expected {}, got {}",
                expected_size,
                data.len()
            )));
        }

        // Validate prefix for secp256r1/k1
        if matches!(curve, ECCurve::Secp256r1 | ECCurve::Secp256k1) {
            if data[0] != 0x02 && data[0] != 0x03 {
                return Err(CryptoError::invalid_point(
                    "Invalid compressed point prefix (expected 0x02 or 0x03)".to_string(),
                ));
            }
        }

        // Validate that the point lies on the curve
        Self::validate_on_curve(curve, &data)?;

        Ok(Self { curve, data })
    }

    /// Validates that the given point data represents a valid point on the specified curve.
    ///
    /// # Security
    /// This is critical for preventing invalid-curve attacks where an attacker provides
    /// a point that is not on the expected curve, potentially leaking private key bits.
    fn validate_on_curve(curve: ECCurve, data: &[u8]) -> CryptoResult<()> {
        match curve {
            ECCurve::Secp256r1 => {
                // Use p256 crate to validate the point
                let encoded_point = p256::EncodedPoint::from_bytes(data).map_err(|e| {
                    CryptoError::invalid_point(format!("Invalid secp256r1 point encoding: {}", e))
                })?;

                // Try to decompress and validate the point is on the curve
                let affine_point: Option<p256::AffinePoint> =
                    p256::AffinePoint::from_encoded_point(&encoded_point).into();

                if affine_point.is_none() {
                    return Err(CryptoError::invalid_point(
                        "Point is not on the secp256r1 curve".to_string(),
                    ));
                }
                Ok(())
            }
            ECCurve::Secp256k1 => {
                // Use k256 crate to validate the point
                let encoded_point = k256::EncodedPoint::from_bytes(data).map_err(|e| {
                    CryptoError::invalid_point(format!("Invalid secp256k1 point encoding: {}", e))
                })?;

                // Try to decompress and validate the point is on the curve
                let affine_point: Option<k256::AffinePoint> =
                    k256::AffinePoint::from_encoded_point(&encoded_point).into();

                if affine_point.is_none() {
                    return Err(CryptoError::invalid_point(
                        "Point is not on the secp256k1 curve".to_string(),
                    ));
                }
                Ok(())
            }
            ECCurve::Ed25519 => {
                // Use ed25519-dalek to validate the point
                let bytes: [u8; 32] = data.try_into().map_err(|_| {
                    CryptoError::invalid_point("Invalid Ed25519 point length".to_string())
                })?;

                // VerifyingKey::from_bytes validates that the point is on the curve
                VerifyingKey::from_bytes(&bytes).map_err(|e| {
                    CryptoError::invalid_point(format!("Invalid Ed25519 point: {}", e))
                })?;
                Ok(())
            }
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
        if matches!(curve, ECCurve::Secp256r1 | ECCurve::Secp256k1) {
            if data[0] != 0x02 && data[0] != 0x03 {
                return Err(CryptoError::invalid_point(
                    "Invalid compressed point prefix (expected 0x02 or 0x03)".to_string(),
                ));
            }
        }

        Ok(Self { curve, data })
    }

    /// Creates an ECPoint from compressed bytes with explicit curve specification.
    ///
    /// # Arguments
    /// * `curve` - The elliptic curve to use
    /// * `data` - Compressed point data
    ///
    /// # Security
    /// Always use this method with explicit curve specification to avoid curve confusion attacks.
    pub fn decode_compressed_with_curve(curve: ECCurve, data: &[u8]) -> CryptoResult<Self> {
        Self::new(curve, data.to_vec())
    }

    /// Creates an ECPoint from compressed bytes, inferring the curve from data length.
    ///
    /// # Warning
    /// This method assumes secp256r1 for 33-byte keys, which may not be correct for secp256k1.
    /// For security-critical code, use `decode_compressed_with_curve` with explicit curve.
    ///
    /// # Deprecated
    /// Use `decode_compressed_with_curve` for explicit curve specification.
    #[deprecated(
        since = "0.7.1",
        note = "Use decode_compressed_with_curve() with explicit curve to avoid curve confusion"
    )]
    pub fn decode_compressed(data: &[u8]) -> CryptoResult<Self> {
        if data.len() == 33 {
            // Assume secp256r1 (Neo's default) - WARNING: may be incorrect for secp256k1
            Self::new(ECCurve::Secp256r1, data.to_vec())
        } else if data.len() == 32 {
            // Assume Ed25519
            Self::new(ECCurve::Ed25519, data.to_vec())
        } else {
            Err(CryptoError::invalid_point(format!(
                "Invalid compressed point length: {}",
                data.len()
            )))
        }
    }

    /// Decodes a secp256r1 (P-256) compressed point.
    pub fn decode_secp256r1(data: &[u8]) -> CryptoResult<Self> {
        Self::new(ECCurve::Secp256r1, data.to_vec())
    }

    /// Decodes a secp256k1 compressed point.
    pub fn decode_secp256k1(data: &[u8]) -> CryptoResult<Self> {
        Self::new(ECCurve::Secp256k1, data.to_vec())
    }

    /// Decodes an Ed25519 public key.
    pub fn decode_ed25519(data: &[u8]) -> CryptoResult<Self> {
        Self::new(ECCurve::Ed25519, data.to_vec())
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
                self.data.len() == 1 && self.data[0] == 0x00
                    || self.data.iter().all(|&b| b == 0)
            }
            ECCurve::Ed25519 => {
                // Ed25519 identity is all zeros
                self.data.iter().all(|&b| b == 0)
            }
        }
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

impl Ord for ECPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare by curve first, then by data
        match self.curve.cmp(&other.curve) {
            Ordering::Equal => self.data.cmp(&other.data),
            other => other,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        // Valid compressed point (prefix 0x02)
        let mut data = vec![0x02];
        data.extend_from_slice(&[0xAA; 32]);
        let point = ECPoint::new(ECCurve::Secp256r1, data.clone()).unwrap();
        assert_eq!(point.curve(), ECCurve::Secp256r1);
        assert_eq!(point.as_bytes(), &data[..]);
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
    }

    #[test]
    fn test_ec_point_decode_compressed() {
        let mut data = vec![0x03];
        data.extend_from_slice(&[0xBB; 32]);
        let point = ECPoint::decode_compressed(&data).unwrap();
        assert_eq!(point.curve(), ECCurve::Secp256r1);
    }

    #[test]
    fn test_ec_point_ordering() {
        let mut data1 = vec![0x02];
        data1.extend_from_slice(&[0x01; 32]);
        let point1 = ECPoint::new(ECCurve::Secp256r1, data1).unwrap();

        let mut data2 = vec![0x02];
        data2.extend_from_slice(&[0x02; 32]);
        let point2 = ECPoint::new(ECCurve::Secp256r1, data2).unwrap();

        assert!(point1 < point2);
    }
}

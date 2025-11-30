//! Elliptic Curve Cryptography for Neo blockchain.
//!
//! Supports secp256r1 (P-256/NIST), secp256k1, and Ed25519 curves.

use crate::error::{CryptoError, CryptoResult};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

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
#[derive(Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ECPoint {
    /// The curve this point belongs to.
    curve: ECCurve,
    /// Compressed representation of the point (33 bytes for secp256r1/k1, 32 for Ed25519).
    data: Vec<u8>,
}

impl ECPoint {
    /// Creates a new ECPoint from compressed bytes.
    ///
    /// # Arguments
    /// * `curve` - The elliptic curve
    /// * `data` - Compressed point data
    ///
    /// # Returns
    /// A new ECPoint or an error if the data is invalid.
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

        Ok(Self { curve, data })
    }

    /// Creates an ECPoint from compressed bytes (33 bytes for secp256r1/k1).
    pub fn decode_compressed(data: &[u8]) -> CryptoResult<Self> {
        if data.len() == 33 {
            // Assume secp256r1 (Neo's default)
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
    pub fn infinity(curve: ECCurve) -> Self {
        let data = vec![0u8; curve.compressed_size()];
        Self { curve, data }
    }

    /// Checks if this point is the infinity point.
    pub fn is_infinity(&self) -> bool {
        self.data.iter().all(|&b| b == 0)
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

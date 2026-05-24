//! Constant-time comparison helpers.

use subtle::ConstantTimeEq;

/// Constant-time comparison utilities for cryptographic operations.
///
/// These functions help prevent timing side-channel attacks when comparing
/// sensitive values like hashes, signatures, or keys.
pub struct ConstantTime;

impl ConstantTime {
    /// Compares two fixed-size byte arrays in constant time.
    #[must_use]
    pub fn eq<const N: usize>(a: &[u8; N], b: &[u8; N]) -> bool {
        a.ct_eq(b).into()
    }

    /// Compares two byte slices in constant time.
    ///
    /// Returns `false` immediately if the slices have different lengths.
    /// Otherwise, performs a constant-time comparison of the contents.
    #[must_use]
    pub fn eq_slice(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }

    /// Compares two 64-byte signatures in constant time.
    #[must_use]
    pub fn eq_signature(a: &[u8; 64], b: &[u8; 64]) -> bool {
        Self::eq(a, b)
    }

    /// Compares two 32-byte hash values in constant time.
    #[must_use]
    pub fn eq_hash256(a: &[u8; 32], b: &[u8; 32]) -> bool {
        Self::eq(a, b)
    }

    /// Compares two 20-byte Hash160 values in constant time.
    #[must_use]
    pub fn eq_hash160(a: &[u8; 20], b: &[u8; 20]) -> bool {
        Self::eq(a, b)
    }
}

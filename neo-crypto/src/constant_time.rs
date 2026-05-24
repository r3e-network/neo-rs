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

#[cfg(test)]
mod tests {
    use super::ConstantTime;

    #[test]
    fn test_constant_time_eq() {
        let a = [0u8; 32];
        let b = [0u8; 32];
        let c = [1u8; 32];

        assert!(ConstantTime::eq(&a, &b));
        assert!(ConstantTime::eq(&a, &a));
        assert!(!ConstantTime::eq(&a, &c));

        let mut d = [0u8; 32];
        d[15] = 1;
        assert!(!ConstantTime::eq(&a, &d));

        d[15] = 0;
        d[0] = 1;
        assert!(!ConstantTime::eq(&a, &d));

        d[0] = 0;
        d[31] = 1;
        assert!(!ConstantTime::eq(&a, &d));
    }

    #[test]
    fn test_constant_time_eq_slice() {
        let a = vec![0u8; 32];
        let b = vec![0u8; 32];
        let c = vec![1u8; 32];
        let d = vec![0u8; 64];

        assert!(ConstantTime::eq_slice(&a, &b));
        assert!(ConstantTime::eq_slice(&a, &a));
        assert!(!ConstantTime::eq_slice(&a, &c));
        assert!(!ConstantTime::eq_slice(&a, &d));
        assert!(ConstantTime::eq_slice(&[], &[]));
        assert!(!ConstantTime::eq_slice(&[], &[0u8]));
    }

    #[test]
    fn test_constant_time_eq_signature() {
        let sig1 = [0u8; 64];
        let sig2 = [0u8; 64];
        let mut sig3 = [0u8; 64];
        sig3[63] = 1;

        assert!(ConstantTime::eq_signature(&sig1, &sig2));
        assert!(!ConstantTime::eq_signature(&sig1, &sig3));
    }

    #[test]
    fn test_constant_time_eq_hash256() {
        let hash1 = [0u8; 32];
        let hash2 = [0u8; 32];
        let mut hash3 = [0u8; 32];
        hash3[31] = 1;

        assert!(ConstantTime::eq_hash256(&hash1, &hash2));
        assert!(!ConstantTime::eq_hash256(&hash1, &hash3));
    }

    #[test]
    fn test_constant_time_eq_hash160() {
        let hash1 = [0u8; 20];
        let hash2 = [0u8; 20];
        let mut hash3 = [0u8; 20];
        hash3[19] = 1;

        assert!(ConstantTime::eq_hash160(&hash1, &hash2));
        assert!(!ConstantTime::eq_hash160(&hash1, &hash3));
    }
}

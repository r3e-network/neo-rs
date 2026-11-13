//! Comprehensive tests for ECCurve

use neo_crypto::ECCurve;

// ========== ECCurve Tests ==========

#[test]
fn test_eccurve_secp256r1() {
    let curve = ECCurve::secp256r1();
    assert_eq!(curve, ECCurve::Secp256r1);
}

#[test]
fn test_eccurve_secp256k1() {
    let curve = ECCurve::secp256k1();
    assert_eq!(curve, ECCurve::Secp256k1);
}

#[test]
fn test_eccurve_ed25519() {
    let curve = ECCurve::ed25519();
    assert_eq!(curve, ECCurve::Ed25519);
}

#[test]
fn test_eccurve_compressed_size() {
    assert_eq!(ECCurve::Secp256r1.compressed_size(), 33);
    assert_eq!(ECCurve::Secp256k1.compressed_size(), 33);
    assert_eq!(ECCurve::Ed25519.compressed_size(), 32);
}

#[test]
fn test_eccurve_uncompressed_size() {
    assert_eq!(ECCurve::Secp256r1.uncompressed_size(), 65);
    assert_eq!(ECCurve::Secp256k1.uncompressed_size(), 65);
    assert_eq!(ECCurve::Ed25519.uncompressed_size(), 32);
}

#[test]
fn test_eccurve_equality() {
    assert_eq!(ECCurve::Secp256r1, ECCurve::Secp256r1);
    assert_ne!(ECCurve::Secp256r1, ECCurve::Secp256k1);
    assert_ne!(ECCurve::Secp256k1, ECCurve::Ed25519);
}

#[test]
fn test_eccurve_clone() {
    let curve = ECCurve::Secp256r1;
    let cloned = curve;
    assert_eq!(curve, cloned);
}

#[test]
fn test_eccurve_copy() {
    let curve = ECCurve::Secp256k1;
    let copied = curve;
    assert_eq!(curve, copied);
}

#[test]
fn test_eccurve_debug() {
    let curve = ECCurve::Secp256r1;
    let debug_str = format!("{:?}", curve);
    assert!(debug_str.contains("Secp256r1"));
}

#[test]
fn test_eccurve_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(ECCurve::Secp256r1);
    set.insert(ECCurve::Secp256k1);
    set.insert(ECCurve::Ed25519);
    assert_eq!(set.len(), 3);
}

#[test]
fn test_eccurve_all_variants() {
    let curves = vec![ECCurve::Secp256r1, ECCurve::Secp256k1, ECCurve::Ed25519];

    assert_eq!(curves.len(), 3);

    for curve in curves {
        assert!(curve.compressed_size() > 0);
        assert!(curve.uncompressed_size() > 0);
    }
}

#[test]
fn test_eccurve_sizes_consistent() {
    // Secp curves should have same sizes
    assert_eq!(
        ECCurve::Secp256r1.compressed_size(),
        ECCurve::Secp256k1.compressed_size()
    );

    assert_eq!(
        ECCurve::Secp256r1.uncompressed_size(),
        ECCurve::Secp256k1.uncompressed_size()
    );

    // Ed25519 is different
    assert_ne!(
        ECCurve::Ed25519.compressed_size(),
        ECCurve::Secp256r1.compressed_size()
    );
}

#[test]
fn test_eccurve_serde() {
    use serde_json;

    let curve = ECCurve::Secp256r1;
    let json = serde_json::to_string(&curve).unwrap();
    let decoded: ECCurve = serde_json::from_str(&json).unwrap();

    assert_eq!(curve, decoded);
}

#[test]
fn test_eccurve_all_serde() {
    use serde_json;

    let curves = vec![ECCurve::Secp256r1, ECCurve::Secp256k1, ECCurve::Ed25519];

    for curve in curves {
        let json = serde_json::to_string(&curve).unwrap();
        let decoded: ECCurve = serde_json::from_str(&json).unwrap();
        assert_eq!(curve, decoded);
    }
}

#[test]
fn test_eccurve_match_all_variants() {
    let curves = [ECCurve::Secp256r1, ECCurve::Secp256k1, ECCurve::Ed25519];

    for curve in &curves {
        match curve {
            ECCurve::Secp256r1 => assert_eq!(curve.compressed_size(), 33),
            ECCurve::Secp256k1 => assert_eq!(curve.compressed_size(), 33),
            ECCurve::Ed25519 => assert_eq!(curve.compressed_size(), 32),
        }
    }
}

#[test]
fn test_eccurve_partial_eq() {
    let r1_a = ECCurve::Secp256r1;
    let r1_b = ECCurve::Secp256r1;
    let k1 = ECCurve::Secp256k1;

    assert!(r1_a == r1_b);
    assert!(r1_a != k1);
}

#[test]
fn test_eccurve_sizes_non_zero() {
    for curve in &[ECCurve::Secp256r1, ECCurve::Secp256k1, ECCurve::Ed25519] {
        assert!(curve.compressed_size() > 0);
        assert!(curve.uncompressed_size() > 0);
        assert!(curve.compressed_size() <= curve.uncompressed_size());
    }
}

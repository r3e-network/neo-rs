//! `NamedCurveHash` - matches C# Neo.SmartContract.Native.NamedCurveHash exactly.

use crate::ecc::ECCurve;
use crate::hash::HashAlgorithm;
use neo_primitives::protocol_enum;

protocol_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    /// Named curve and hash algorithm combination (matches C# `NamedCurveHash`).
    pub NamedCurveHash {
        /// secp256k1 with SHA256.
        Secp256k1SHA256 = 0x16 => "secp256k1SHA256",
        /// secp256r1 with SHA256.
        Secp256r1SHA256 = 0x17 => "secp256r1SHA256",
        /// secp256k1 with Keccak256. C# NamedCurveHash.secp256k1Keccak256 = 122.
        Secp256k1Keccak256 = 0x7A => "secp256k1Keccak256",
        /// secp256r1 with Keccak256. C# NamedCurveHash.secp256r1Keccak256 = 123.
        Secp256r1Keccak256 = 0x7B => "secp256r1Keccak256",
    }
}

impl NamedCurveHash {
    /// Gets the curve type.
    #[must_use]
    pub const fn curve(self) -> ECCurve {
        match self {
            Self::Secp256k1SHA256 | Self::Secp256k1Keccak256 => ECCurve::Secp256k1,
            Self::Secp256r1SHA256 | Self::Secp256r1Keccak256 => ECCurve::Secp256r1,
        }
    }

    /// Gets the hash algorithm.
    #[must_use]
    pub const fn hash_algorithm(self) -> HashAlgorithm {
        match self {
            Self::Secp256k1SHA256 | Self::Secp256r1SHA256 => HashAlgorithm::Sha256,
            Self::Secp256k1Keccak256 | Self::Secp256r1Keccak256 => HashAlgorithm::Keccak256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_curve_hash_values() {
        assert_eq!(NamedCurveHash::Secp256k1SHA256.to_byte(), 0x16);
        assert_eq!(NamedCurveHash::Secp256r1SHA256.to_byte(), 0x17);
        assert_eq!(NamedCurveHash::Secp256k1Keccak256.to_byte(), 122);
        assert_eq!(NamedCurveHash::Secp256r1Keccak256.to_byte(), 123);
    }

    #[test]
    fn test_named_curve_hash_from_byte() {
        assert_eq!(
            NamedCurveHash::from_byte(0x16),
            Some(NamedCurveHash::Secp256k1SHA256)
        );
        assert_eq!(
            NamedCurveHash::from_byte(0x17),
            Some(NamedCurveHash::Secp256r1SHA256)
        );
        assert_eq!(
            NamedCurveHash::from_byte(122),
            Some(NamedCurveHash::Secp256k1Keccak256)
        );
        assert_eq!(
            NamedCurveHash::from_byte(123),
            Some(NamedCurveHash::Secp256r1Keccak256)
        );
        assert_eq!(NamedCurveHash::from_byte(0x00), None);
        // The old (incorrect) Keccak byte values must NOT decode.
        assert_eq!(NamedCurveHash::from_byte(0x18), None);
        assert_eq!(NamedCurveHash::from_byte(0x19), None);
    }

    #[test]
    fn test_named_curve_hash_roundtrip() {
        for nch in [
            NamedCurveHash::Secp256k1SHA256,
            NamedCurveHash::Secp256r1SHA256,
            NamedCurveHash::Secp256k1Keccak256,
            NamedCurveHash::Secp256r1Keccak256,
        ] {
            let byte = nch.to_byte();
            let recovered = NamedCurveHash::from_byte(byte);
            assert_eq!(recovered, Some(nch));
        }
    }

    #[test]
    fn test_named_curve_hash_curve() {
        assert_eq!(NamedCurveHash::Secp256k1SHA256.curve(), ECCurve::Secp256k1);
        assert_eq!(NamedCurveHash::Secp256r1SHA256.curve(), ECCurve::Secp256r1);
        assert_eq!(
            NamedCurveHash::Secp256k1Keccak256.curve(),
            ECCurve::Secp256k1
        );
        assert_eq!(
            NamedCurveHash::Secp256r1Keccak256.curve(),
            ECCurve::Secp256r1
        );
    }

    #[test]
    fn test_named_curve_hash_algorithm() {
        assert_eq!(
            NamedCurveHash::Secp256k1SHA256.hash_algorithm(),
            HashAlgorithm::Sha256
        );
        assert_eq!(
            NamedCurveHash::Secp256r1SHA256.hash_algorithm(),
            HashAlgorithm::Sha256
        );
        assert_eq!(
            NamedCurveHash::Secp256k1Keccak256.hash_algorithm(),
            HashAlgorithm::Keccak256
        );
        assert_eq!(
            NamedCurveHash::Secp256r1Keccak256.hash_algorithm(),
            HashAlgorithm::Keccak256
        );
    }

    #[test]
    fn test_named_curve_hash_display() {
        assert_eq!(
            NamedCurveHash::Secp256k1SHA256.to_string(),
            "secp256k1SHA256"
        );
        assert_eq!(
            NamedCurveHash::Secp256r1SHA256.to_string(),
            "secp256r1SHA256"
        );
        assert_eq!(
            NamedCurveHash::Secp256k1Keccak256.to_string(),
            "secp256k1Keccak256"
        );
        assert_eq!(
            NamedCurveHash::Secp256r1Keccak256.to_string(),
            "secp256r1Keccak256"
        );
    }

    #[test]
    fn test_named_curve_hash_serde_uses_protocol_byte() {
        let serialized = serde_json::to_string(&NamedCurveHash::Secp256r1Keccak256).unwrap();
        assert_eq!(serialized, "123");

        let deserialized: NamedCurveHash = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, NamedCurveHash::Secp256r1Keccak256);
        assert!(serde_json::from_str::<NamedCurveHash>("21").is_err());
    }
}

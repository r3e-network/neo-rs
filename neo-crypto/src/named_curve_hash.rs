//! NamedCurveHash - matches C# Neo.SmartContract.Native.NamedCurveHash exactly.

use crate::ecc::ECCurve;
use crate::hash::HashAlgorithm;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Named curve and hash algorithm combination (matches C# NamedCurveHash).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NamedCurveHash {
    /// secp256k1 with SHA256.
    Secp256k1SHA256 = 0x16,
    /// secp256r1 with SHA256.
    Secp256r1SHA256 = 0x17,
    /// secp256k1 with Keccak256.
    Secp256k1Keccak256 = 0x18,
    /// secp256r1 with Keccak256.
    Secp256r1Keccak256 = 0x19,
}

impl NamedCurveHash {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x16 => Some(NamedCurveHash::Secp256k1SHA256),
            0x17 => Some(NamedCurveHash::Secp256r1SHA256),
            0x18 => Some(NamedCurveHash::Secp256k1Keccak256),
            0x19 => Some(NamedCurveHash::Secp256r1Keccak256),
            _ => None,
        }
    }

    /// Gets the curve type.
    pub fn curve(self) -> ECCurve {
        match self {
            NamedCurveHash::Secp256k1SHA256 | NamedCurveHash::Secp256k1Keccak256 => {
                ECCurve::Secp256k1
            }
            NamedCurveHash::Secp256r1SHA256 | NamedCurveHash::Secp256r1Keccak256 => {
                ECCurve::Secp256r1
            }
        }
    }

    /// Gets the hash algorithm.
    pub fn hash_algorithm(self) -> HashAlgorithm {
        match self {
            NamedCurveHash::Secp256k1SHA256 | NamedCurveHash::Secp256r1SHA256 => {
                HashAlgorithm::Sha256
            }
            NamedCurveHash::Secp256k1Keccak256 | NamedCurveHash::Secp256r1Keccak256 => {
                HashAlgorithm::Keccak256
            }
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            NamedCurveHash::Secp256k1SHA256 => "secp256k1SHA256",
            NamedCurveHash::Secp256r1SHA256 => "secp256r1SHA256",
            NamedCurveHash::Secp256k1Keccak256 => "secp256k1Keccak256",
            NamedCurveHash::Secp256r1Keccak256 => "secp256r1Keccak256",
        }
    }
}

impl fmt::Display for NamedCurveHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for NamedCurveHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for NamedCurveHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        NamedCurveHash::from_byte(byte).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid named curve hash byte: {byte}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_curve_hash_values() {
        assert_eq!(NamedCurveHash::Secp256k1SHA256.to_byte(), 0x16);
        assert_eq!(NamedCurveHash::Secp256r1SHA256.to_byte(), 0x17);
        assert_eq!(NamedCurveHash::Secp256k1Keccak256.to_byte(), 0x18);
        assert_eq!(NamedCurveHash::Secp256r1Keccak256.to_byte(), 0x19);
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
            NamedCurveHash::from_byte(0x18),
            Some(NamedCurveHash::Secp256k1Keccak256)
        );
        assert_eq!(
            NamedCurveHash::from_byte(0x19),
            Some(NamedCurveHash::Secp256r1Keccak256)
        );
        assert_eq!(NamedCurveHash::from_byte(0x00), None);
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
}

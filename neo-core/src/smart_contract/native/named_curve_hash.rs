//! NamedCurveHash - matches C# Neo.SmartContract.Native.NamedCurveHash exactly

use crate::cryptography::HashAlgorithm;

/// Named curve and hash algorithm combination (matches C# NamedCurveHash)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedCurveHash {
    /// secp256k1 with SHA256
    Secp256k1SHA256 = 0x16,
    
    /// secp256r1 with SHA256
    Secp256r1SHA256 = 0x17,
    
    /// secp256k1 with Keccak256
    Secp256k1Keccak256 = 0x18,
    
    /// secp256r1 with Keccak256
    Secp256r1Keccak256 = 0x19,
}

impl NamedCurveHash {
    /// Creates from byte value
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x16 => Some(NamedCurveHash::Secp256k1SHA256),
            0x17 => Some(NamedCurveHash::Secp256r1SHA256),
            0x18 => Some(NamedCurveHash::Secp256k1Keccak256),
            0x19 => Some(NamedCurveHash::Secp256r1Keccak256),
            _ => None,
        }
    }
    
    /// Gets the curve type
    pub fn curve(&self) -> EllipticCurve {
        match self {
            NamedCurveHash::Secp256k1SHA256 | NamedCurveHash::Secp256k1Keccak256 => {
                EllipticCurve::Secp256k1
            }
            NamedCurveHash::Secp256r1SHA256 | NamedCurveHash::Secp256r1Keccak256 => {
                EllipticCurve::Secp256r1
            }
        }
    }
    
    /// Gets the hash algorithm
    pub fn hash_algorithm(&self) -> HashAlgorithm {
        match self {
            NamedCurveHash::Secp256k1SHA256 | NamedCurveHash::Secp256r1SHA256 => {
                HashAlgorithm::Sha256
            }
            NamedCurveHash::Secp256k1Keccak256 | NamedCurveHash::Secp256r1Keccak256 => {
                HashAlgorithm::Keccak256
            }
        }
    }
}

/// Elliptic curve type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EllipticCurve {
    Secp256k1,
    Secp256r1,
}

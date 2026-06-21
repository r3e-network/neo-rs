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
#[path = "tests/named_curve_hash.rs"]
mod tests;

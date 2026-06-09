//! BLS12-381 curve-point operations backing Neo's `CryptoLib` native methods
//! (`bls12381Serialize` / `bls12381Deserialize` / `bls12381Equal` / `…Add` /
//! `…Mul` / `…Pairing`).
//!
//! This is distinct from the BLS *signature* helpers in [`crate::bls12381`]
//! (which use `blst::min_sig`): the `CryptoLib` methods operate on raw group
//! elements (G1, G2, and the target group Gt), so they need the low-level
//! point API rather than the signature scheme.
//!
//! C# parity reference: `Neo.SmartContract.Native.CryptoLib.BLS12_381` +
//! `Neo.Cryptography.BLS12_381` (a managed port of the zkcrypto `bls12_381`
//! crate). The compressed encodings here (G1 = 48 bytes, G2 = 96 bytes) follow
//! the standard ZCash/IETF format that both `blst` and that crate implement, so
//! they are byte-identical — verified against the `UT_CryptoLib` vectors.
//!
//! Scope: this module currently implements the G1/G2 **encoding** surface
//! (deserialize / serialize / equality), which is the standardized, low-risk
//! part. The target-group `Gt` element (576-byte `fp12` serialization) and the
//! arithmetic operations (`add` / `mul` / `pairing`) are intentionally deferred
//! — they are byte-format-subtle and must be added one operation at a time,
//! each checked against its `UT_CryptoLib` vector.

use crate::error::{CryptoError, CryptoResult};
use blst::{
    blst_p1_affine, blst_p1_affine_compress, blst_p1_affine_in_g1, blst_p1_affine_is_equal,
    blst_p1_uncompress, blst_p2_affine, blst_p2_affine_compress, blst_p2_affine_in_g2,
    blst_p2_affine_is_equal, blst_p2_uncompress, BLST_ERROR,
};

/// Compressed length of a G1 point (matches C# `G1Affine.ToCompressed`).
pub const G1_COMPRESSED_SIZE: usize = 48;
/// Compressed length of a G2 point (matches C# `G2Affine.ToCompressed`).
pub const G2_COMPRESSED_SIZE: usize = 96;

/// A BLS12-381 curve point in one of the curve's groups.
///
/// Mirrors the runtime objects Neo's `CryptoLib` wraps in an `InteropInterface`
/// (`G1Affine` / `G2Affine` / `Gt`). The `Gt` variant is not yet modelled here
/// (see the module docs).
#[derive(Clone)]
pub enum Bls12381Point {
    /// A point in the G1 group (48-byte compressed encoding).
    G1(blst_p1_affine),
    /// A point in the G2 group (96-byte compressed encoding).
    G2(blst_p2_affine),
}

impl Bls12381Point {
    /// Deserializes a compressed BLS12-381 point: 48 bytes → G1, 96 bytes → G2.
    ///
    /// Matches C# `G1Affine.FromCompressed` / `G2Affine.FromCompressed`:
    /// rejects a malformed encoding, a point not on the curve, or a point
    /// outside the prime-order subgroup.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::invalid_point`] for an unsupported length, a bad
    /// encoding, or a point not in the correct subgroup.
    pub fn deserialize(data: &[u8]) -> CryptoResult<Self> {
        match data.len() {
            G1_COMPRESSED_SIZE => {
                let mut affine = blst_p1_affine::default();
                // SAFETY: `data` is exactly 48 bytes and `affine` is a valid
                // out-param; `blst_p1_uncompress` reads 48 bytes and writes the
                // affine point, returning a status code we check.
                let status = unsafe { blst_p1_uncompress(&mut affine, data.as_ptr()) };
                if status != BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_point(
                        "invalid BLS12-381 G1 point encoding",
                    ));
                }
                // SAFETY: `affine` was initialised by the successful uncompress.
                if !unsafe { blst_p1_affine_in_g1(&affine) } {
                    return Err(CryptoError::invalid_point(
                        "BLS12-381 G1 point is not in the prime-order subgroup",
                    ));
                }
                Ok(Bls12381Point::G1(affine))
            }
            G2_COMPRESSED_SIZE => {
                let mut affine = blst_p2_affine::default();
                // SAFETY: as above, for the 96-byte G2 encoding.
                let status = unsafe { blst_p2_uncompress(&mut affine, data.as_ptr()) };
                if status != BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_point(
                        "invalid BLS12-381 G2 point encoding",
                    ));
                }
                // SAFETY: `affine` was initialised by the successful uncompress.
                if !unsafe { blst_p2_affine_in_g2(&affine) } {
                    return Err(CryptoError::invalid_point(
                        "BLS12-381 G2 point is not in the prime-order subgroup",
                    ));
                }
                Ok(Bls12381Point::G2(affine))
            }
            other => Err(CryptoError::invalid_point(format!(
                "invalid BLS12-381 point length: {other} (expected {G1_COMPRESSED_SIZE} for G1 or {G2_COMPRESSED_SIZE} for G2)"
            ))),
        }
    }

    /// Serializes the point to its compressed form (G1 → 48 bytes, G2 → 96
    /// bytes), matching C# `G1Affine.ToCompressed` / `G2Affine.ToCompressed`.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Bls12381Point::G1(affine) => {
                let mut out = [0u8; G1_COMPRESSED_SIZE];
                // SAFETY: `out` is exactly 48 bytes; `blst_p1_affine_compress`
                // writes 48 bytes from the valid affine point.
                unsafe { blst_p1_affine_compress(out.as_mut_ptr(), affine) };
                out.to_vec()
            }
            Bls12381Point::G2(affine) => {
                let mut out = [0u8; G2_COMPRESSED_SIZE];
                // SAFETY: `out` is exactly 96 bytes.
                unsafe { blst_p2_affine_compress(out.as_mut_ptr(), affine) };
                out.to_vec()
            }
        }
    }

    /// Returns `true` only when both points are in the same group and equal,
    /// matching C# `Bls12381Equal` (a G1/G2 type mismatch is never equal).
    #[must_use]
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            // SAFETY: both affines are valid (constructed via `deserialize`).
            (Bls12381Point::G1(a), Bls12381Point::G1(b)) => unsafe { blst_p1_affine_is_equal(a, b) },
            (Bls12381Point::G2(a), Bls12381Point::G2(b)) => unsafe { blst_p2_affine_is_equal(a, b) },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The canonical BLS12-381 generators, from UT_CryptoLib (s_g1Hex / s_g2Hex).
    const G1_GEN: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb";
    const G2_GEN: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";
    // notG1 / notG2 from UT_CryptoLib: well-formed length, not valid points.
    const NOT_G1: &str = "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NOT_G2: &str = "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn g1_g2_compressed_round_trip_matches_csharp_vectors() {
        let g1_bytes = hex::decode(G1_GEN).unwrap();
        let g1 = Bls12381Point::deserialize(&g1_bytes).expect("G1 generator deserializes");
        assert!(matches!(g1, Bls12381Point::G1(_)));
        assert_eq!(g1.serialize(), g1_bytes, "G1 compressed round-trip");

        let g2_bytes = hex::decode(G2_GEN).unwrap();
        let g2 = Bls12381Point::deserialize(&g2_bytes).expect("G2 generator deserializes");
        assert!(matches!(g2, Bls12381Point::G2(_)));
        assert_eq!(g2.serialize(), g2_bytes, "G2 compressed round-trip");
    }

    #[test]
    fn equals_matches_group_and_point() {
        let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
        let g1_again = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
        let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();

        assert!(g1.equals(&g1_again), "same G1 point is equal");
        assert!(!g1.equals(&g2), "G1 vs G2 is never equal");
    }

    #[test]
    fn rejects_invalid_and_wrong_length() {
        // C# TestNotG1 / TestNotG2: well-formed length but not valid points.
        assert!(Bls12381Point::deserialize(&hex::decode(NOT_G1).unwrap()).is_err());
        assert!(Bls12381Point::deserialize(&hex::decode(NOT_G2).unwrap()).is_err());
        // Unsupported lengths.
        assert!(Bls12381Point::deserialize(&[]).is_err());
        assert!(Bls12381Point::deserialize(&[0u8; 32]).is_err());
        assert!(Bls12381Point::deserialize(&[0u8; 576]).is_err()); // Gt not yet supported
    }
}

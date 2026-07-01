#![allow(unsafe_code)]

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
//! crate). The G1/G2 compressed encodings (48 / 96 bytes) follow the standard
//! ZCash/IETF format; the Gt encoding is the 576-byte form (12 `Fp`
//! coefficients, each 48-byte big-endian, in the `Fp12 = Fp6 × Fp6`,
//! `Fp6 = Fp2 × 3`, `Fp2 = Fp × 2` nesting order). All encodings are verified
//! byte-exact against the in-repo `UT_CryptoLib` vectors.
//!
//! Scope: the full `CryptoLib` BLS12-381 arithmetic surface — G1/G2/Gt
//! serialize / deserialize / equal, the group operation (`Bls12381Add` — point
//! addition for G1/G2, `Fp12` multiplication for Gt), scalar multiplication
//! (`Bls12381Mul`), and the optimal-ate pairing (`Bls12381Pairing`). Every
//! operation is verified byte-exact against the in-repo `UT_CryptoLib` vectors
//! (pairing / add / mul). What remains is the
//! `InteropInterface`-over-native-dispatch seam that wires these into
//! `crypto_lib.rs` so they become VM-callable.

use crate::error::{CryptoError, CryptoResult};
use blst::{
    BLST_ERROR, blst_bendian_from_fp, blst_final_exp, blst_fp, blst_fp_from_bendian, blst_fp12,
    blst_fp12_inverse, blst_fp12_is_equal, blst_fp12_mul, blst_fp12_one, blst_fp12_sqr,
    blst_miller_loop, blst_p1, blst_p1_add_or_double, blst_p1_affine, blst_p1_affine_compress,
    blst_p1_affine_in_g1, blst_p1_affine_is_equal, blst_p1_cneg, blst_p1_from_affine, blst_p1_mult,
    blst_p1_to_affine, blst_p1_uncompress, blst_p2, blst_p2_add_or_double, blst_p2_affine,
    blst_p2_affine_compress, blst_p2_affine_in_g2, blst_p2_affine_is_equal, blst_p2_cneg,
    blst_p2_from_affine, blst_p2_mult, blst_p2_to_affine, blst_p2_uncompress,
};

/// Byte length of a BLS12-381 scalar multiplier (matches C# `Scalar.FromBytes`).
pub const SCALAR_SIZE: usize = 32;

/// Compressed length of a G1 point (matches C# `G1Affine.ToCompressed`).
pub const G1_COMPRESSED_SIZE: usize = 48;
/// Compressed length of a G2 point (matches C# `G2Affine.ToCompressed`).
pub const G2_COMPRESSED_SIZE: usize = 96;
/// Serialized length of a Gt element (matches C# `Gt.ToArray`): 12 `Fp` × 48B.
pub const GT_SIZE: usize = 576;

/// A BLS12-381 curve point in one of the curve's groups.
///
/// Mirrors the runtime objects Neo's `CryptoLib` wraps in an `InteropInterface`
/// (`G1Affine` / `G2Affine` / `Gt`).
// The Gt variant (576-byte `Fp12`) is intentionally inline: this enum is a
// short-lived operand for a single CryptoLib call (deserialize → operate →
// serialize), never bulk-stored, and boxing would force `&**` on every raw
// `blst_*` FFI pointer in the hot path.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub enum Bls12381Point {
    /// A point in the G1 group (48-byte compressed encoding).
    G1(blst_p1_affine),
    /// A point in the G2 group (96-byte compressed encoding).
    G2(blst_p2_affine),
    /// An element of the target group Gt (576-byte `Fp12` encoding).
    Gt(blst_fp12),
}

/// Reads a 48-byte big-endian `Fp` from `bytes` (which must be ≥ 48 long).
fn fp_from_be(bytes: &[u8]) -> blst_fp {
    let mut fp = blst_fp::default();
    // SAFETY: `bytes` has ≥ 48 bytes; `blst_fp_from_bendian` reads exactly 48.
    unsafe { blst_fp_from_bendian(&mut fp, bytes.as_ptr()) };
    fp
}

/// Parses a 576-byte Gt encoding into an `Fp12`.
///
/// Coefficient walk order matches C# `Gt.ToArray` (a port of the zkcrypto
/// `bls12_381` crate): the `Fp12 = Fp6 × Fp6`, `Fp6 = Fp2 × 3`, `Fp2 = Fp × 2`
/// tower, written highest-degree coefficient first at every level — i.e. the
/// `fp6[i].fp2[j].fp[k]` indices descend (`i: 1→0`, `j: 2→0`, `k: 1→0`), each
/// coefficient a 48-byte big-endian `Fp`. Verified byte-exact against the
/// `UT_CryptoLib` pairing / add / mul vectors.
fn gt_from_bytes(data: &[u8]) -> blst_fp12 {
    let mut fp12 = blst_fp12::default();
    let mut idx = 0usize;
    for i in (0..2).rev() {
        for j in (0..3).rev() {
            for k in (0..2).rev() {
                fp12.fp6[i].fp2[j].fp[k] = fp_from_be(&data[idx * 48..]);
                idx += 1;
            }
        }
    }
    fp12
}

/// Serializes an `Fp12` (Gt element) to its 576-byte form (same walk order as
/// [`gt_from_bytes`]).
fn gt_to_bytes(fp12: &blst_fp12) -> Vec<u8> {
    let mut out = vec![0u8; GT_SIZE];
    let mut idx = 0usize;
    for i in (0..2).rev() {
        for j in (0..3).rev() {
            for k in (0..2).rev() {
                // SAFETY: `out[idx*48..]` has ≥ 48 bytes for idx < 12.
                unsafe {
                    blst_bendian_from_fp(out[idx * 48..].as_mut_ptr(), &fp12.fp6[i].fp2[j].fp[k]);
                }
                idx += 1;
            }
        }
    }
    out
}

/// Adds two affine G1 points (via projective addition).
fn g1_add(a: &blst_p1_affine, b: &blst_p1_affine) -> blst_p1_affine {
    let mut ap = blst_p1::default();
    let mut bp = blst_p1::default();
    let mut sum = blst_p1::default();
    let mut out = blst_p1_affine::default();
    // SAFETY: all operands are valid affine/projective points.
    unsafe {
        blst_p1_from_affine(&mut ap, a);
        blst_p1_from_affine(&mut bp, b);
        blst_p1_add_or_double(&mut sum, &ap, &bp);
        blst_p1_to_affine(&mut out, &sum);
    }
    out
}

/// Adds two affine G2 points (via projective addition).
fn g2_add(a: &blst_p2_affine, b: &blst_p2_affine) -> blst_p2_affine {
    let mut ap = blst_p2::default();
    let mut bp = blst_p2::default();
    let mut sum = blst_p2::default();
    let mut out = blst_p2_affine::default();
    // SAFETY: all operands are valid affine/projective points.
    unsafe {
        blst_p2_from_affine(&mut ap, a);
        blst_p2_from_affine(&mut bp, b);
        blst_p2_add_or_double(&mut sum, &ap, &bp);
        blst_p2_to_affine(&mut out, &sum);
    }
    out
}

/// Multiplies an affine G1 point by a 32-byte little-endian scalar, negating the
/// result point when `neg` (`p * (r - X) = -(p * X)` for an `r`-order point, so
/// this matches C# `p * (neg ? -Scalar : Scalar)`).
fn g1_mul(p: &blst_p1_affine, scalar_le: &[u8; SCALAR_SIZE], neg: bool) -> blst_p1_affine {
    let mut pp = blst_p1::default();
    let mut prod = blst_p1::default();
    let mut out = blst_p1_affine::default();
    // SAFETY: `p` is a valid affine point; scalar is exactly 32 bytes.
    unsafe {
        blst_p1_from_affine(&mut pp, p);
        blst_p1_mult(&mut prod, &pp, scalar_le.as_ptr(), SCALAR_SIZE * 8);
        blst_p1_cneg(&mut prod, neg);
        blst_p1_to_affine(&mut out, &prod);
    }
    out
}

/// G2 counterpart of [`g1_mul`].
fn g2_mul(p: &blst_p2_affine, scalar_le: &[u8; SCALAR_SIZE], neg: bool) -> blst_p2_affine {
    let mut pp = blst_p2::default();
    let mut prod = blst_p2::default();
    let mut out = blst_p2_affine::default();
    // SAFETY: `p` is a valid affine point; scalar is exactly 32 bytes.
    unsafe {
        blst_p2_from_affine(&mut pp, p);
        blst_p2_mult(&mut prod, &pp, scalar_le.as_ptr(), SCALAR_SIZE * 8);
        blst_p2_cneg(&mut prod, neg);
        blst_p2_to_affine(&mut out, &prod);
    }
    out
}

/// Raises a Gt element (`Fp12`) to a 32-byte little-endian scalar power via
/// MSB-first square-and-multiply, inverting when `neg`. Gt is written
/// multiplicatively, so `gt * X` is `gt^X` and `gt * (-X)` is `(gt^X)^-1`
/// (matches C# `Gt * Scalar`).
fn gt_pow(base: &blst_fp12, scalar_le: &[u8; SCALAR_SIZE], neg: bool) -> blst_fp12 {
    // SAFETY: `blst_fp12_one` returns a pointer to a valid static `Fp12`.
    let mut acc = unsafe { *blst_fp12_one() };
    for byte_idx in (0..SCALAR_SIZE).rev() {
        let b = scalar_le[byte_idx];
        for bit in (0..8).rev() {
            let mut sq = blst_fp12::default();
            // SAFETY: `acc` and `base` are valid `Fp12` values.
            unsafe { blst_fp12_sqr(&mut sq, &acc) };
            acc = sq;
            if (b >> bit) & 1 == 1 {
                let mut prod = blst_fp12::default();
                // SAFETY: operands are valid `Fp12` values.
                unsafe { blst_fp12_mul(&mut prod, &acc, base) };
                acc = prod;
            }
        }
    }
    if neg {
        let mut inv = blst_fp12::default();
        // SAFETY: `acc` is a valid non-zero `Fp12` (a Gt-subgroup element).
        unsafe { blst_fp12_inverse(&mut inv, &acc) };
        acc = inv;
    }
    acc
}

impl Bls12381Point {
    /// Deserializes a BLS12-381 point: 48 bytes → G1, 96 bytes → G2, 576 bytes → Gt.
    ///
    /// Matches C# `bls12381Deserialize`: rejects a malformed/off-curve/non-subgroup
    /// G1 or G2 encoding. (Gt is parsed structurally, as on the C# side.)
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::invalid_point`] for an unsupported length, a bad
    /// encoding, or a G1/G2 point not in the correct subgroup.
    pub fn deserialize(data: &[u8]) -> CryptoResult<Self> {
        match data.len() {
            G1_COMPRESSED_SIZE => {
                let mut affine = blst_p1_affine::default();
                // SAFETY: `data` is exactly 48 bytes; status is checked.
                let status = unsafe { blst_p1_uncompress(&mut affine, data.as_ptr()) };
                if status != BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_point(
                        "invalid BLS12-381 G1 point encoding",
                    ));
                }
                // SAFETY: `affine` initialised by the successful uncompress.
                if !unsafe { blst_p1_affine_in_g1(&affine) } {
                    return Err(CryptoError::invalid_point(
                        "BLS12-381 G1 point is not in the prime-order subgroup",
                    ));
                }
                Ok(Bls12381Point::G1(affine))
            }
            G2_COMPRESSED_SIZE => {
                let mut affine = blst_p2_affine::default();
                // SAFETY: `data` is exactly 96 bytes; status is checked.
                let status = unsafe { blst_p2_uncompress(&mut affine, data.as_ptr()) };
                if status != BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_point(
                        "invalid BLS12-381 G2 point encoding",
                    ));
                }
                // SAFETY: `affine` initialised by the successful uncompress.
                if !unsafe { blst_p2_affine_in_g2(&affine) } {
                    return Err(CryptoError::invalid_point(
                        "BLS12-381 G2 point is not in the prime-order subgroup",
                    ));
                }
                Ok(Bls12381Point::G2(affine))
            }
            GT_SIZE => Ok(Bls12381Point::Gt(gt_from_bytes(data))),
            other => Err(CryptoError::invalid_point(format!(
                "invalid BLS12-381 point length: {other} (expected {G1_COMPRESSED_SIZE}/{G2_COMPRESSED_SIZE}/{GT_SIZE})"
            ))),
        }
    }

    /// Serializes the point (G1 → 48 bytes, G2 → 96 bytes, Gt → 576 bytes),
    /// matching C# `bls12381Serialize`.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Bls12381Point::G1(affine) => {
                let mut out = [0u8; G1_COMPRESSED_SIZE];
                // SAFETY: `out` is exactly 48 bytes.
                unsafe { blst_p1_affine_compress(out.as_mut_ptr(), affine) };
                out.to_vec()
            }
            Bls12381Point::G2(affine) => {
                let mut out = [0u8; G2_COMPRESSED_SIZE];
                // SAFETY: `out` is exactly 96 bytes.
                unsafe { blst_p2_affine_compress(out.as_mut_ptr(), affine) };
                out.to_vec()
            }
            Bls12381Point::Gt(fp12) => gt_to_bytes(fp12),
        }
    }

    /// The group operation (`bls12381Add`): point addition for G1/G2, and `Fp12`
    /// multiplication for Gt (the target group is written additively but its
    /// operation is multiplication, matching zkcrypto / Neo).
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::invalid_point`] when the operands are in different
    /// groups.
    pub fn add(&self, other: &Self) -> CryptoResult<Self> {
        match (self, other) {
            (Bls12381Point::G1(a), Bls12381Point::G1(b)) => Ok(Bls12381Point::G1(g1_add(a, b))),
            (Bls12381Point::G2(a), Bls12381Point::G2(b)) => Ok(Bls12381Point::G2(g2_add(a, b))),
            (Bls12381Point::Gt(a), Bls12381Point::Gt(b)) => {
                let mut out = blst_fp12::default();
                // SAFETY: both `Fp12` operands are valid.
                unsafe { blst_fp12_mul(&mut out, a, b) };
                Ok(Bls12381Point::Gt(out))
            }
            _ => Err(CryptoError::invalid_point(
                "BLS12-381 add: operands are in different groups",
            )),
        }
    }

    /// Scalar multiplication (`bls12381Mul`): multiplies the point by a 32-byte
    /// little-endian scalar, negating the scalar first when `neg`. G1/G2 use
    /// point scalar multiplication; Gt uses `Fp12` exponentiation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::invalid_point`] when `scalar_le` is not exactly
    /// [`SCALAR_SIZE`] bytes.
    pub fn mul(&self, scalar_le: &[u8], neg: bool) -> CryptoResult<Self> {
        let scalar: &[u8; SCALAR_SIZE] = scalar_le.try_into().map_err(|_| {
            CryptoError::invalid_point(format!(
                "BLS12-381 mul: scalar must be {SCALAR_SIZE} bytes, got {}",
                scalar_le.len()
            ))
        })?;
        Ok(match self {
            Bls12381Point::G1(p) => Bls12381Point::G1(g1_mul(p, scalar, neg)),
            Bls12381Point::G2(p) => Bls12381Point::G2(g2_mul(p, scalar, neg)),
            Bls12381Point::Gt(p) => Bls12381Point::Gt(gt_pow(p, scalar, neg)),
        })
    }

    /// The optimal-ate pairing `e(g1, g2) → Gt` (`bls12381Pairing`): a Miller
    /// loop followed by the final exponentiation.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::invalid_point`] unless `self` is a G1 point and
    /// `other` is a G2 point (matching the C# argument typing).
    pub fn pairing(&self, other: &Self) -> CryptoResult<Self> {
        match (self, other) {
            (Bls12381Point::G1(g1), Bls12381Point::G2(g2)) => {
                let mut miller = blst_fp12::default();
                let mut out = blst_fp12::default();
                // SAFETY: `g1`/`g2` are valid affine points in their subgroups.
                unsafe {
                    blst_miller_loop(&mut miller, g2, g1);
                    blst_final_exp(&mut out, &miller);
                }
                Ok(Bls12381Point::Gt(out))
            }
            _ => Err(CryptoError::invalid_point(
                "BLS12-381 pairing: expected a G1 point and a G2 point",
            )),
        }
    }

    /// Returns `true` only when both points are in the same group and equal,
    /// matching C# `Bls12381Equal` (a cross-group comparison is never equal).
    #[must_use]
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            // SAFETY: operands are valid (constructed via `deserialize`/`add`).
            (Bls12381Point::G1(a), Bls12381Point::G1(b)) => unsafe {
                blst_p1_affine_is_equal(a, b)
            },
            (Bls12381Point::G2(a), Bls12381Point::G2(b)) => unsafe {
                blst_p2_affine_is_equal(a, b)
            },
            (Bls12381Point::Gt(a), Bls12381Point::Gt(b)) => unsafe { blst_fp12_is_equal(a, b) },
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/curves/bls12381_point.rs"]
mod tests;

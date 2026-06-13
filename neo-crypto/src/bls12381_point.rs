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
mod tests {
    use super::*;

    // Canonical BLS12-381 generators + a Gt element, from UT_CryptoLib
    // (s_g1Hex / s_g2Hex / s_gtHex). GT_ADD_HEX = TestBls12381Add expected output.
    const G1_GEN: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb";
    const G2_GEN: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";
    const GT_HEX: &str = "0f41e58663bf08cf068672cbd01a7ec73baca4d72ca93544deff686bfd6df543d48eaa24afe47e1efde449383b67663104c581234d086a9902249b64728ffd21a189e87935a954051c7cdba7b3872629a4fafc05066245cb9108f0242d0fe3ef03350f55a7aefcd3c31b4fcb6ce5771cc6a0e9786ab5973320c806ad360829107ba810c5a09ffdd9be2291a0c25a99a211b8b424cd48bf38fcef68083b0b0ec5c81a93b330ee1a677d0d15ff7b984e8978ef48881e32fac91b93b47333e2ba5706fba23eb7c5af0d9f80940ca771b6ffd5857baaf222eb95a7d2809d61bfe02e1bfd1b68ff02f0b8102ae1c2d5d5ab1a19f26337d205fb469cd6bd15c3d5a04dc88784fbb3d0b2dbdea54d43b2b73f2cbb12d58386a8703e0f948226e47ee89d018107154f25a764bd3c79937a45b84546da634b8f6be14a8061e55cceba478b23f7dacaa35c8ca78beae9624045b4b601b2f522473d171391125ba84dc4007cfbf2f8da752f7c74185203fcca589ac719c34dffbbaad8431dad1c1fb597aaa5193502b86edb8857c273fa075a50512937e0794e1e65a7617c90d8bd66065b1fffe51d7a579973b1315021ec3c19934f1368bb445c7c2d209703f239689ce34c0378a68e72a6b3b216da0e22a5031b54ddff57309396b38c881c4c849ec23e87089a1c5b46e5110b86750ec6a532348868a84045483c92b7af5af689452eafabf1a8943e50439f1d59882a98eaa0170f1250ebd871fc0a92a7b2d83168d0d727272d441befa15c503dd8e90ce98db3e7b6d194f60839c508a84305aaca1789b6";
    const GT_ADD_HEX: &str = "079ab7b345eb23c944c957a36a6b74c37537163d4cbf73bad9751de1dd9c68ef72cb21447e259880f72a871c3eda1b0c017f1c95cf79b22b459599ea57e613e00cb75e35de1f837814a93b443c54241015ac9761f8fb20a44512ff5cfc04ac7f0f6b8b52b2b5d0661cbf232820a257b8c5594309c01c2a45e64c6a7142301e4fb36e6e16b5a85bd2e437599d103c3ace06d8046c6b3424c4cd2d72ce98d279f2290a28a87e8664cb0040580d0c485f34df45267f8c215dcbcd862787ab555c7e113286dee21c9c63a458898beb35914dc8daaac453441e7114b21af7b5f47d559879d477cf2a9cbd5b40c86becd071280900410bb2751d0a6af0fe175dcf9d864ecaac463c6218745b543f9e06289922434ee446030923a3e4c4473b4e3b1914081abd33a78d31eb8d4c1bb3baab0529bb7baf1103d848b4cead1a8e0aa7a7b260fbe79c67dbe41ca4d65ba8a54a72b61692a61ce5f4d7a093b2c46aa4bca6c4a66cf873d405ebc9c35d8aa639763720177b23beffaf522d5e41d3c5310ea3331409cebef9ef393aa00f2ac64673675521e8fc8fddaf90976e607e62a740ac59c3dddf95a6de4fba15beb30c43d4e3f803a3734dbeb064bf4bc4a03f945a4921e49d04ab8d45fd753a28b8fa082616b4b17bbcb685e455ff3bf8f60c3bd32a0c185ef728cf41a1b7b700b7e445f0b372bc29e370bc227d443c70ae9dbcf73fee8acedbd317a286a53266562d817269c004fb0f149dd925d2c590a960936763e519c2b62e14c7759f96672cd852194325904197b0b19c6b528ab33566946af39b";
    const GT_MUL_POS_HEX: &str = "18b2db6b3286baea116ccad8f5554d170a69b329a6de5b24c50b8834965242001a1c58089fd872b211acd3263897fa660b117248d69d8ac745283a3e6a4ccec607f6cf7cedee919575d4b7c8ae14c36001f76be5fca50adc296ef8df4926fa7f0b55a75f255fe61fc2da7cffe56adc8775aaab54c50d0c4952ad919d90fb0eb221c41abb9f2352a11be2d7f176abe41e0e30afb34fc2ce16136de66900d92068f30011e9882c0a56e7e7b30f08442be9e58d093e1888151136259d059fb539210d635bc491d5244a16ca28fdcf10546ec0f7104d3a419ddc081ba30ecb0cd2289010c2d385946229b7a9735adc82736914fe61ad26c6c38b787775de3b939105de055f8d7004358272a0823f6f1787a7abb6c3c59c8c9cbd1674ac900512632818cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f926fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf83157930bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d00440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef80f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6";
    const GT_MUL_NEG_HEX: &str = "014e367f06f92bb039aedcdd4df65fc05a0d985b4ca6b79aa2254a6c605eb424048fa7f6117b8d4da8522cd9c767b0450eef9fa162e25bd305f36d77d8fede115c807c0805968129f15c1ad8489c32c41cb49418b4aef52390900720b6d8b02c0eab6a8b1420007a88412ab65de0d04feecca0302e7806761483410365b5e771fce7e5431230ad5e9e1c280e8953c68d0bd06236e9bd188437adc14d42728c6e7177399b6b5908687f491f91ee6cca3a391ef6c098cbeaee83d962fa604a718a0c9db625a7aac25034517eb8743b5868a3803b37b94374e35f152f922ba423fb8e9b3d2b2bbf9dd602558ca5237d37420502b03d12b9230ed2a431d807b81bd18671ebf78380dd3cf490506187996e7c72f53c3914c76342a38a536ffaed478318cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f926fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf83157930bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d00440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef80f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6";
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
    fn gt_round_trip_and_add_match_csharp_vectors() {
        let gt_bytes = hex::decode(GT_HEX).unwrap();
        assert_eq!(gt_bytes.len(), GT_SIZE);
        let gt = Bls12381Point::deserialize(&gt_bytes).expect("Gt deserializes");
        assert!(matches!(gt, Bls12381Point::Gt(_)));
        // Self-consistency: deserialize -> serialize is identity.
        assert_eq!(gt.serialize(), gt_bytes, "Gt round-trip");
        // Semantic gate: bls12381Add(gt, gt) == gt*gt in Fp12, matching C#.
        let sum = gt.add(&gt).expect("Gt add");
        assert_eq!(
            hex::encode(sum.serialize()),
            GT_ADD_HEX,
            "Gt add matches the C# TestBls12381Add vector"
        );
    }

    #[test]
    fn pairing_matches_csharp_vector() {
        // C# TestBls12381Pairing: e(g1, g2) serializes to s_gtHex.
        let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
        let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();
        let gt = g1.pairing(&g2).expect("pairing g1 x g2");
        assert!(matches!(gt, Bls12381Point::Gt(_)));
        assert_eq!(hex::encode(gt.serialize()), GT_HEX, "e(g1,g2) == s_gtHex");
        // Argument typing is enforced (C# accepts only G1 then G2).
        assert!(g2.pairing(&g1).is_err(), "pairing rejects G2 x G1 ordering");
    }

    #[test]
    fn gt_scalar_mul_matches_csharp_vectors() {
        // C# TestBls12381Mul: scalar = 32-byte LE with data[0]=0x03 (i.e. 3).
        let mut scalar = [0u8; SCALAR_SIZE];
        scalar[0] = 0x03;
        let gt = Bls12381Point::deserialize(&hex::decode(GT_HEX).unwrap()).unwrap();

        let pos = gt.mul(&scalar, false).expect("gt * 3");
        assert_eq!(hex::encode(pos.serialize()), GT_MUL_POS_HEX, "gt * 3");

        let neg = gt.mul(&scalar, true).expect("gt * -3");
        assert_eq!(hex::encode(neg.serialize()), GT_MUL_NEG_HEX, "gt * -3");

        // gt*3 and gt*(-3) are inverses: their product is the Gt identity.
        let prod = pos.add(&neg).expect("gt*3 + gt*-3");
        let one = Bls12381Point::deserialize(&hex::decode(GT_HEX).unwrap())
            .unwrap()
            .mul(&[0u8; SCALAR_SIZE], false)
            .expect("gt * 0 = identity");
        assert!(prod.equals(&one), "gt*3 * gt*-3 == identity");

        // Wrong scalar length is rejected.
        assert!(gt.mul(&[0u8; 31], false).is_err());
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
    fn add_rejects_cross_group() {
        let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
        let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();
        assert!(g1.add(&g2).is_err(), "adding G1 + G2 is rejected");
    }

    #[test]
    fn rejects_invalid_and_wrong_length() {
        // C# TestNotG1 / TestNotG2: well-formed length but not valid points.
        assert!(Bls12381Point::deserialize(&hex::decode(NOT_G1).unwrap()).is_err());
        assert!(Bls12381Point::deserialize(&hex::decode(NOT_G2).unwrap()).is_err());
        // Unsupported lengths.
        assert!(Bls12381Point::deserialize(&[]).is_err());
        assert!(Bls12381Point::deserialize(&[0u8; 100]).is_err());
    }
}

//! CryptoLib BLS12-381 point helpers.
//!
//! Point arguments and results cross the native boundary as canonical bytes;
//! this module keeps the pure point operations separate from engine dispatch.

use super::CryptoLib;
use neo_crypto::Bls12381Point;
use neo_error::{CoreError, CoreResult};

impl CryptoLib {
    /// Deserializes the `idx`-th argument as a BLS12-381 point, faulting on a
    /// missing argument or a malformed/off-curve/wrong-subgroup encoding (C#
    /// `InteropInterface` binding + `FromCompressed`/`FromBytes` throw -> VM fault).
    fn bls_point(method: &str, args: &[Vec<u8>], idx: usize) -> CoreResult<Bls12381Point> {
        let bytes = args.get(idx).ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} is missing argument {idx}"))
        })?;
        Bls12381Point::deserialize(bytes)
            .map_err(|e| CoreError::invalid_operation(format!("CryptoLib::{method}: {e}")))
    }

    /// Pure BLS12-381 `CryptoLib` dispatch (serialize / deserialize / equal /
    /// add / mul / pairing), split out so it can be unit-tested without an
    /// [`neo_execution::ApplicationEngine`]. Point arguments arrive as their
    /// canonical encoding (the dispatcher unwraps the `Bls12381Interop`
    /// operands to raw bytes); point results are returned as canonical bytes for
    /// the dispatcher to re-wrap as a `Bls12381Interop`. `bls12381Equal`
    /// returns a single boolean byte.
    ///
    /// Returns `Ok(None)` when `method` is not a BLS method (so the caller can
    /// fall through to the hash methods).
    pub(super) fn bls12381_method(method: &str, args: &[Vec<u8>]) -> Option<CoreResult<Vec<u8>>> {
        let result = match method {
            // Serialize takes a point (InteropInterface) and returns its bytes;
            // the operand already arrives canonical, so round-tripping it normalizes.
            "bls12381Serialize" => Self::bls_point(method, args, 0).map(|p| p.serialize()),
            // Deserialize validates raw bytes into a point; the dispatcher
            // re-wraps the canonical encoding as an interop object.
            "bls12381Deserialize" => Self::bls_point(method, args, 0).map(|p| p.serialize()),
            "bls12381Equal" => Self::bls_point(method, args, 0).and_then(|a| {
                let b = Self::bls_point(method, args, 1)?;
                // C# `Bls12381Equal` throws `ArgumentException("BLS12-381 type
                // mismatch")` -> VM FAULT for a cross-group comparison (e.g. G1
                // vs G2/Gt); only a same-group pair returns a boolean.
                if !a.same_group(&b) {
                    return Err(CoreError::invalid_operation(
                        "CryptoLib::bls12381Equal: BLS12-381 type mismatch",
                    ));
                }
                Ok(vec![u8::from(a.equals(&b))])
            }),
            "bls12381Add" => Self::bls_point(method, args, 0).and_then(|a| {
                let b = Self::bls_point(method, args, 1)?;
                a.add(&b).map(|sum| sum.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Add: {e}"))
                })
            }),
            "bls12381Mul" => Self::bls_point(method, args, 0).and_then(|p| {
                let mul = args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("CryptoLib::bls12381Mul is missing the multiplier")
                })?;
                // The `neg` flag (3rd arg) is a VM Boolean: any non-zero byte is true.
                let neg = args.get(2).is_some_and(|b| b.iter().any(|x| *x != 0));
                p.mul(mul, neg).map(|out| out.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Mul: {e}"))
                })
            }),
            "bls12381Pairing" => Self::bls_point(method, args, 0).and_then(|g1| {
                let g2 = Self::bls_point(method, args, 1)?;
                g1.pairing(&g2).map(|gt| gt.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Pairing: {e}"))
                })
            }),
            _ => return None,
        };
        Some(result)
    }
}

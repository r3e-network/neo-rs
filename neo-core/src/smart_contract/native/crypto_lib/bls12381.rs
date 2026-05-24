use super::CryptoLib;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

use blst::{
    blst_fp, blst_fp12, blst_p1, blst_p1_affine, blst_p2, blst_p2_affine, blst_scalar, BLST_ERROR,
};

const BLS_INTEROP_G1_AFFINE: u8 = 0x01;
const BLS_INTEROP_G1_PROJECTIVE: u8 = 0x02;
const BLS_INTEROP_G2_AFFINE: u8 = 0x03;
const BLS_INTEROP_G2_PROJECTIVE: u8 = 0x04;
const BLS_INTEROP_GT: u8 = 0x05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bls12381Group {
    G1,
    G2,
    Gt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bls12381Kind {
    G1Affine,
    G1Projective,
    G2Affine,
    G2Projective,
    Gt,
}

impl Bls12381Kind {
    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            BLS_INTEROP_G1_AFFINE => Some(Self::G1Affine),
            BLS_INTEROP_G1_PROJECTIVE => Some(Self::G1Projective),
            BLS_INTEROP_G2_AFFINE => Some(Self::G2Affine),
            BLS_INTEROP_G2_PROJECTIVE => Some(Self::G2Projective),
            BLS_INTEROP_GT => Some(Self::Gt),
            _ => None,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::G1Affine => BLS_INTEROP_G1_AFFINE,
            Self::G1Projective => BLS_INTEROP_G1_PROJECTIVE,
            Self::G2Affine => BLS_INTEROP_G2_AFFINE,
            Self::G2Projective => BLS_INTEROP_G2_PROJECTIVE,
            Self::Gt => BLS_INTEROP_GT,
        }
    }

    fn group(self) -> Bls12381Group {
        match self {
            Self::G1Affine | Self::G1Projective => Bls12381Group::G1,
            Self::G2Affine | Self::G2Projective => Bls12381Group::G2,
            Self::Gt => Bls12381Group::Gt,
        }
    }

    fn expected_len(self) -> usize {
        match self {
            Self::G1Affine | Self::G1Projective => 48,
            Self::G2Affine | Self::G2Projective => 96,
            Self::Gt => 576,
        }
    }

    fn interface_type(self) -> &'static str {
        match self {
            Self::G1Affine => "Bls12381G1Affine",
            Self::G1Projective => "Bls12381G1Projective",
            Self::G2Affine => "Bls12381G2Affine",
            Self::G2Projective => "Bls12381G2Projective",
            Self::Gt => "Bls12381Gt",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Bls12381Interop {
    kind: Bls12381Kind,
    bytes: Vec<u8>,
}

impl Bls12381Interop {
    fn new(kind: Bls12381Kind, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != kind.expected_len() {
            return Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            ));
        }
        Ok(Self { kind, bytes })
    }

    pub(crate) fn from_encoded_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::native_contract(
                "Invalid BLS12-381 interop payload".to_string(),
            ));
        }
        let kind = Bls12381Kind::from_tag(data[0])
            .ok_or_else(|| Error::native_contract("Invalid BLS12-381 interop payload"))?;
        let bytes = data[1..].to_vec();
        Self::new(kind, bytes)
    }

    pub(crate) fn to_encoded_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len() + 1);
        out.push(self.kind.tag());
        out.extend_from_slice(&self.bytes);
        out
    }

    fn kind(&self) -> Bls12381Kind {
        self.kind
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl VmInteropInterface for Bls12381Interop {
    fn interface_type(&self) -> &str {
        self.kind.interface_type()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CryptoLib {
    /// BLS12-381 point addition
    pub(super) fn bls12381_add(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Add requires two point arguments".to_string(),
            ));
        }

        let x = Bls12381Interop::from_encoded_bytes(&args[0])?;
        let y = Bls12381Interop::from_encoded_bytes(&args[1])?;

        if x.kind().group() != y.kind().group() {
            return Err(Error::native_contract(
                "BLS12-381 type mismatch".to_string(),
            ));
        }

        let bytes = match x.kind().group() {
            Bls12381Group::G1 => {
                let p1 = self.deserialize_g1(x.bytes())?;
                let p2 = self.deserialize_g1(y.bytes())?;
                let result = self.g1_add(&p1, &p2);
                self.serialize_g1(&result)?
            }
            Bls12381Group::G2 => {
                let p1 = self.deserialize_g2(x.bytes())?;
                let p2 = self.deserialize_g2(y.bytes())?;
                let result = self.g2_add(&p1, &p2);
                self.serialize_g2(&result)?
            }
            Bls12381Group::Gt => {
                let p1 = self.deserialize_gt(x.bytes())?;
                let p2 = self.deserialize_gt(y.bytes())?;
                let result = self.gt_add(&p1, &p2);
                self.serialize_gt(&result)?
            }
        };

        let output_kind = match x.kind().group() {
            Bls12381Group::G1 => Bls12381Kind::G1Projective,
            Bls12381Group::G2 => Bls12381Kind::G2Projective,
            Bls12381Group::Gt => Bls12381Kind::Gt,
        };
        Ok(Bls12381Interop::new(output_kind, bytes)?.to_encoded_bytes())
    }

    /// BLS12-381 equality check
    pub(super) fn bls12381_equal(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Equal requires two point arguments".to_string(),
            ));
        }

        let x = Bls12381Interop::from_encoded_bytes(&args[0])?;
        let y = Bls12381Interop::from_encoded_bytes(&args[1])?;

        if x.kind() != y.kind() {
            return Err(Error::native_contract(
                "BLS12-381 type mismatch".to_string(),
            ));
        }

        match x.kind().group() {
            Bls12381Group::G1 => {
                let p1 = self.deserialize_g1(x.bytes())?;
                let p2 = self.deserialize_g1(y.bytes())?;
                // SAFETY: p1, p2 are valid G1 affine points from `deserialize_g1`.
                let equal = unsafe { blst::blst_p1_affine_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
            Bls12381Group::G2 => {
                let p1 = self.deserialize_g2(x.bytes())?;
                let p2 = self.deserialize_g2(y.bytes())?;
                // SAFETY: p1, p2 are valid G2 affine points from `deserialize_g2`.
                let equal = unsafe { blst::blst_p2_affine_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
            Bls12381Group::Gt => {
                let p1 = self.deserialize_gt(x.bytes())?;
                let p2 = self.deserialize_gt(y.bytes())?;
                // SAFETY: p1, p2 are valid Fp12 values from `deserialize_gt`.
                let equal = unsafe { blst::blst_fp12_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
        }
    }

    /// BLS12-381 scalar multiplication
    pub(super) fn bls12381_mul(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Mul requires point and scalar arguments".to_string(),
            ));
        }

        let point = Bls12381Interop::from_encoded_bytes(&args[0])?;
        let scalar = &args[1];
        let neg = args
            .get(2)
            .map(|v| !v.is_empty() && v[0] != 0)
            .unwrap_or(false);

        if scalar.len() != 32 {
            return Err(Error::native_contract(
                "Invalid BLS12-381 scalar size".to_string(),
            ));
        }

        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(scalar);

        let bytes = match point.kind().group() {
            Bls12381Group::G1 => {
                let p = self.deserialize_g1(point.bytes())?;
                let result = self.g1_mul(&p, &scalar_bytes, neg);
                self.serialize_g1(&result)?
            }
            Bls12381Group::G2 => {
                let p = self.deserialize_g2(point.bytes())?;
                let result = self.g2_mul(&p, &scalar_bytes, neg);
                self.serialize_g2(&result)?
            }
            Bls12381Group::Gt => {
                let p = self.deserialize_gt(point.bytes())?;
                let result = self.gt_mul(&p, &scalar_bytes, neg);
                self.serialize_gt(&result)?
            }
        };

        let output_kind = match point.kind().group() {
            Bls12381Group::G1 => Bls12381Kind::G1Projective,
            Bls12381Group::G2 => Bls12381Kind::G2Projective,
            Bls12381Group::Gt => Bls12381Kind::Gt,
        };
        Ok(Bls12381Interop::new(output_kind, bytes)?.to_encoded_bytes())
    }

    /// BLS12-381 pairing operation
    pub(super) fn bls12381_pairing(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Pairing requires G1 and G2 point arguments".to_string(),
            ));
        }

        let g1_point = Bls12381Interop::from_encoded_bytes(&args[0])?;
        let g2_point = Bls12381Interop::from_encoded_bytes(&args[1])?;

        if g1_point.kind().group() != Bls12381Group::G1
            || g2_point.kind().group() != Bls12381Group::G2
        {
            return Err(Error::native_contract(
                "BLS12-381 type mismatch".to_string(),
            ));
        }

        let p1 = self.deserialize_g1(g1_point.bytes())?;
        let p2 = self.deserialize_g2(g2_point.bytes())?;

        let bytes = self.compute_pairing(&p1, &p2)?;
        Ok(Bls12381Interop::new(Bls12381Kind::Gt, bytes)?.to_encoded_bytes())
    }

    /// BLS12-381 point serialization
    pub(super) fn bls12381_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Serialize requires point argument".to_string(),
            ));
        }
        let interop = Bls12381Interop::from_encoded_bytes(&args[0])?;
        let data = interop.bytes();
        match interop.kind().group() {
            Bls12381Group::G1 => {
                let point = self.deserialize_g1(data)?;
                let mut proj = blst_p1::default();
                // SAFETY: `point` is a validated G1 affine point from `deserialize_g1`.
                unsafe {
                    blst::blst_p1_from_affine(&mut proj, &point);
                }
                self.serialize_g1(&proj)
            }
            Bls12381Group::G2 => {
                let point = self.deserialize_g2(data)?;
                let mut proj = blst_p2::default();
                // SAFETY: `point` is a validated G2 affine point from `deserialize_g2`.
                unsafe {
                    blst::blst_p2_from_affine(&mut proj, &point);
                }
                self.serialize_g2(&proj)
            }
            Bls12381Group::Gt => {
                let point = self.deserialize_gt(data)?;
                self.serialize_gt(&point)
            }
        }
    }

    /// BLS12-381 point deserialization
    pub(super) fn bls12381_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Deserialize requires bytes argument".to_string(),
            ));
        }

        let data = &args[0];

        let (kind, bytes) = match data.len() {
            48 => {
                let _ = self.deserialize_g1(data)?;
                (Bls12381Kind::G1Affine, data.clone())
            }
            96 => {
                let _ = self.deserialize_g2(data)?;
                (Bls12381Kind::G2Affine, data.clone())
            }
            576 => {
                let _ = self.deserialize_gt(data)?;
                (Bls12381Kind::Gt, data.clone())
            }
            _ => {
                return Err(Error::native_contract(
                    "Invalid BLS12-381 serialized point size".to_string(),
                ))
            }
        };

        Ok(Bls12381Interop::new(kind, bytes)?.to_encoded_bytes())
    }

    // BLS12-381 helper functions
    //
    // SAFETY NOTES for all BLS12-381 FFI calls:
    // - The blst library is a well-audited cryptographic library used in Ethereum 2.0
    // - All pointer arguments are valid: we pass references to stack-allocated or heap-allocated
    //   Rust values that outlive the FFI call
    // - Output buffers are pre-allocated with correct sizes (48 bytes for G1, 96 bytes for G2,
    //   576 bytes for Fp12)
    // - The blst library handles invalid curve points gracefully by returning error codes
    //   rather than causing undefined behavior

    fn deserialize_g1(&self, data: &[u8]) -> Result<blst_p1_affine> {
        let mut point = blst_p1_affine::default();
        // SAFETY: `point` is a valid mutable reference, `data.as_ptr()` points to valid memory
        // for at least 48 bytes (caller must ensure this). blst returns an error code for
        // invalid input rather than causing UB.
        unsafe {
            let result = blst::blst_p1_uncompress(&mut point, data.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(Error::native_contract("Invalid G1 point"));
            }
            if blst::blst_p1_affine_is_inf(&point) || !blst::blst_p1_affine_in_g1(&point) {
                return Err(Error::native_contract(
                    "G1 point not in correct subgroup".to_string(),
                ));
            }
        }
        Ok(point)
    }

    fn deserialize_g2(&self, data: &[u8]) -> Result<blst_p2_affine> {
        let mut point = blst_p2_affine::default();
        // SAFETY: `point` is a valid mutable reference, `data.as_ptr()` points to valid memory
        // for at least 96 bytes (caller must ensure this). blst returns an error code for
        // invalid input rather than causing UB.
        unsafe {
            let result = blst::blst_p2_uncompress(&mut point, data.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(Error::native_contract("Invalid G2 point"));
            }
            if blst::blst_p2_affine_is_inf(&point) || !blst::blst_p2_affine_in_g2(&point) {
                return Err(Error::native_contract(
                    "G2 point not in correct subgroup".to_string(),
                ));
            }
        }
        Ok(point)
    }

    fn serialize_g1(&self, point: &blst_p1) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 48];
        // SAFETY: `out` is pre-allocated with exactly 48 bytes (G1 compressed size),
        // `point` is a valid reference to a blst_p1 structure.
        unsafe {
            blst::blst_p1_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    fn serialize_g2(&self, point: &blst_p2) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 96];
        // SAFETY: `out` is pre-allocated with exactly 96 bytes (G2 compressed size),
        // `point` is a valid reference to a blst_p2 structure.
        unsafe {
            blst::blst_p2_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    fn serialize_gt(&self, point: &blst_fp12) -> Result<Vec<u8>> {
        const FP_SIZE: usize = 48;
        const FP2_SIZE: usize = FP_SIZE * 2;
        const FP6_SIZE: usize = FP2_SIZE * 3;
        const FP12_SIZE: usize = FP6_SIZE * 2;

        let mut out = vec![0u8; FP12_SIZE];
        let mut offset = 0usize;

        for fp6_index in [1usize, 0usize] {
            for fp2_index in [2usize, 1usize, 0usize] {
                for fp_index in [1usize, 0usize] {
                    let fp = &point.fp6[fp6_index].fp2[fp2_index].fp[fp_index];
                    // SAFETY: `out` slice is pre-sized to 48 bytes for each field element.
                    unsafe {
                        blst::blst_bendian_from_fp(out[offset..offset + FP_SIZE].as_mut_ptr(), fp);
                    }
                    offset += FP_SIZE;
                }
            }
        }

        Ok(out)
    }

    fn g1_add(&self, p1: &blst_p1_affine, p2: &blst_p1_affine) -> blst_p1 {
        let mut result = blst_p1::default();
        let mut p1_proj = blst_p1::default();
        let mut p2_proj = blst_p1::default();
        // SAFETY: All arguments are valid references to properly initialized blst structures.
        // The blst library performs curve point addition without UB for any valid input.
        unsafe {
            blst::blst_p1_from_affine(&mut p1_proj, p1);
            blst::blst_p1_from_affine(&mut p2_proj, p2);
            blst::blst_p1_add(&mut result, &p1_proj, &p2_proj);
        }
        result
    }

    fn g2_add(&self, p1: &blst_p2_affine, p2: &blst_p2_affine) -> blst_p2 {
        let mut result = blst_p2::default();
        let mut p1_proj = blst_p2::default();
        let mut p2_proj = blst_p2::default();
        // SAFETY: All arguments are valid references to properly initialized blst structures.
        // The blst library performs curve point addition without UB for any valid input.
        unsafe {
            blst::blst_p2_from_affine(&mut p1_proj, p1);
            blst::blst_p2_from_affine(&mut p2_proj, p2);
            blst::blst_p2_add(&mut result, &p1_proj, &p2_proj);
        }
        result
    }

    fn gt_add(&self, p1: &blst_fp12, p2: &blst_fp12) -> blst_fp12 {
        let mut result = blst_fp12::default();
        // SAFETY: All arguments are valid references to properly initialized blst structures.
        // GT group operation corresponds to multiplication in Fp12.
        unsafe {
            blst::blst_fp12_mul(&mut result, p1, p2);
        }
        result
    }

    fn g1_mul(&self, p: &blst_p1_affine, scalar: &[u8; 32], neg: bool) -> blst_p1 {
        let mut result = blst_p1::default();
        let mut p_proj = blst_p1::default();
        let mut scalar_val = blst_scalar::default();

        // SAFETY: All arguments are valid references. `scalar` is exactly 32 bytes as required
        // by blst_scalar_from_lendian. The blst library handles scalar multiplication safely.
        unsafe {
            blst::blst_p1_from_affine(&mut p_proj, p);
            blst::blst_scalar_from_lendian(&mut scalar_val, scalar.as_ptr());

            // blst_p1_mult expects a projective point
            blst::blst_p1_mult(&mut result, &p_proj, scalar_val.b.as_ptr(), 256);

            // Handle negation by negating the result
            if neg {
                blst::blst_p1_cneg(&mut result, true);
            }
        }
        result
    }

    fn g2_mul(&self, p: &blst_p2_affine, scalar: &[u8; 32], neg: bool) -> blst_p2 {
        let mut result = blst_p2::default();
        let mut p_proj = blst_p2::default();
        let mut scalar_val = blst_scalar::default();

        // SAFETY: All arguments are valid references. `scalar` is exactly 32 bytes as required
        // by blst_scalar_from_lendian. The blst library handles scalar multiplication safely.
        unsafe {
            blst::blst_p2_from_affine(&mut p_proj, p);
            blst::blst_scalar_from_lendian(&mut scalar_val, scalar.as_ptr());

            blst::blst_p2_mult(&mut result, &p_proj, scalar_val.b.as_ptr(), 256);

            if neg {
                blst::blst_p2_cneg(&mut result, true);
            }
        }
        result
    }

    fn gt_mul(&self, p: &blst_fp12, scalar: &[u8; 32], neg: bool) -> blst_fp12 {
        // SAFETY: `blst_fp12_one` returns a pointer to a static constant (the identity element).
        let mut result = unsafe { *blst::blst_fp12_one() };
        let base = *p;

        for byte in scalar.iter().rev() {
            for bit in (0..8).rev() {
                // SAFETY: result and base are valid blst_fp12 values.
                unsafe {
                    blst::blst_fp12_sqr(&mut result, &result);
                }
                if (byte >> bit) & 1 == 1 {
                    // SAFETY: result and base are valid blst_fp12 values.
                    unsafe {
                        blst::blst_fp12_mul(&mut result, &result, &base);
                    }
                }
            }
        }

        if neg {
            // SAFETY: result is a valid blst_fp12 value.
            unsafe {
                blst::blst_fp12_inverse(&mut result, &result);
            }
        }

        result
    }

    fn deserialize_gt(&self, data: &[u8]) -> Result<blst_fp12> {
        const FP_SIZE: usize = 48;
        const FP2_SIZE: usize = FP_SIZE * 2;
        const FP6_SIZE: usize = FP2_SIZE * 3;
        const FP12_SIZE: usize = FP6_SIZE * 2;

        if data.len() != FP12_SIZE {
            return Err(Error::native_contract(
                "Invalid BLS12-381 GT point size".to_string(),
            ));
        }

        let mut point = blst_fp12::default();
        let mut offset = 0usize;

        for fp6_index in [1usize, 0usize] {
            for fp2_index in [2usize, 1usize, 0usize] {
                for fp_index in [1usize, 0usize] {
                    let slice = &data[offset..offset + FP_SIZE];
                    Self::read_fp_from_bendian(
                        &mut point.fp6[fp6_index].fp2[fp2_index].fp[fp_index],
                        slice,
                    )?;
                    offset += FP_SIZE;
                }
            }
        }

        Ok(point)
    }

    fn read_fp_from_bendian(target: &mut blst_fp, data: &[u8]) -> Result<()> {
        const FP_SIZE: usize = 48;
        if data.len() != FP_SIZE {
            return Err(Error::native_contract(
                "Invalid BLS12-381 field element size".to_string(),
            ));
        }

        // SAFETY: `data` length is validated to be exactly 48 bytes (FP_SIZE) above.
        // `target` is a valid mutable reference to a blst_fp struct.
        unsafe {
            blst::blst_fp_from_bendian(target, data.as_ptr());
        }

        let mut check = [0u8; FP_SIZE];
        // SAFETY: `target` was just written by `blst_fp_from_bendian`. `check` is a
        // stack-allocated 48-byte array matching the expected output size.
        unsafe {
            blst::blst_bendian_from_fp(check.as_mut_ptr(), target);
        }

        if check != data {
            return Err(Error::native_contract(
                "Invalid BLS12-381 GT point".to_string(),
            ));
        }

        Ok(())
    }

    fn compute_pairing(&self, g1: &blst_p1_affine, g2: &blst_p2_affine) -> Result<Vec<u8>> {
        let mut result = blst_fp12::default();
        // SAFETY: All arguments are valid references to properly initialized blst structures.
        // The Miller loop and final exponentiation are deterministic operations.
        unsafe {
            blst::blst_miller_loop(&mut result, g2, g1);
            blst::blst_final_exp(&mut result, &result);
        }
        self.serialize_gt(&result)
    }
}

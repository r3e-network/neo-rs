use super::*;
use blst::{
    BLST_ERROR, blst_fp, blst_fp12, blst_p1, blst_p1_affine, blst_p2, blst_p2_affine, blst_scalar,
};

impl CryptoLib {
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

    pub(super) fn deserialize_g1(&self, data: &[u8]) -> Result<blst_p1_affine> {
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

    pub(super) fn deserialize_g2(&self, data: &[u8]) -> Result<blst_p2_affine> {
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

    pub(super) fn serialize_g1(&self, point: &blst_p1) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 48];
        // SAFETY: `out` is pre-allocated with exactly 48 bytes (G1 compressed size),
        // `point` is a valid reference to a blst_p1 structure.
        unsafe {
            blst::blst_p1_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    pub(super) fn serialize_g2(&self, point: &blst_p2) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 96];
        // SAFETY: `out` is pre-allocated with exactly 96 bytes (G2 compressed size),
        // `point` is a valid reference to a blst_p2 structure.
        unsafe {
            blst::blst_p2_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    pub(super) fn serialize_gt(&self, point: &blst_fp12) -> Result<Vec<u8>> {
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

    pub(super) fn g1_add(&self, p1: &blst_p1_affine, p2: &blst_p1_affine) -> blst_p1 {
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

    pub(super) fn g2_add(&self, p1: &blst_p2_affine, p2: &blst_p2_affine) -> blst_p2 {
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

    pub(super) fn gt_add(&self, p1: &blst_fp12, p2: &blst_fp12) -> blst_fp12 {
        let mut result = blst_fp12::default();
        // SAFETY: All arguments are valid references to properly initialized blst structures.
        // GT group operation corresponds to multiplication in Fp12.
        unsafe {
            blst::blst_fp12_mul(&mut result, p1, p2);
        }
        result
    }

    pub(super) fn g1_mul(&self, p: &blst_p1_affine, scalar: &[u8; 32], neg: bool) -> blst_p1 {
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

    pub(super) fn g2_mul(&self, p: &blst_p2_affine, scalar: &[u8; 32], neg: bool) -> blst_p2 {
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

    pub(super) fn gt_mul(&self, p: &blst_fp12, scalar: &[u8; 32], neg: bool) -> blst_fp12 {
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

    pub(super) fn deserialize_gt(&self, data: &[u8]) -> Result<blst_fp12> {
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

    pub(super) fn read_fp_from_bendian(target: &mut blst_fp, data: &[u8]) -> Result<()> {
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

    pub(super) fn compute_pairing(
        &self,
        g1: &blst_p1_affine,
        g2: &blst_p2_affine,
    ) -> Result<Vec<u8>> {
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

//! BLS12-381 signature helpers for Neo.
//!
//! This module wraps the widely used `blst` crate while keeping Neo's exact
//! domain separation tag and encoding choices isolated from general crypto
//! utilities.

use crate::error::{CryptoError, CryptoResult};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

/// BLS12-381 operations using the `blst` crate.
///
/// Neo uses the "minimal-signature-size" scheme:
/// - Private key: scalar (32 bytes)
/// - Public key: G2 point (96 bytes compressed)
/// - Signature: G1 point (48 bytes compressed)
pub struct Bls12381Crypto;

/// Domain Separation Tag for Neo BLS12-381 signatures.
///
/// This must match the C# implementation exactly for cross-compatibility.
const NEO_BLS_DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

impl Bls12381Crypto {
    fn validate_private_key(private_key: &[u8; 32]) -> CryptoResult<blst::blst_scalar> {
        use blst::blst_scalar;

        if private_key.iter().all(|b| *b == 0) {
            return Err(CryptoError::invalid_key(
                "Invalid private key: scalar cannot be zero",
            ));
        }

        let mut sk_scalar = blst_scalar::default();
        // SAFETY: `blst_scalar_from_lendian` reads exactly 32 bytes from `private_key`,
        // which is guaranteed to be a valid 32-byte slice. `blst_scalar_fr_check` only
        // reads the initialized `sk_scalar`. Both are pure FFI calls with no aliasing.
        unsafe {
            blst::blst_scalar_from_lendian(&mut sk_scalar, private_key.as_ptr());
            if !blst::blst_scalar_fr_check(&sk_scalar) {
                return Err(CryptoError::invalid_key(
                    "Invalid private key: scalar not in Fr field",
                ));
            }
        }
        Ok(sk_scalar)
    }

    /// Generates a new random private key using cryptographically secure RNG.
    #[must_use]
    pub fn generate_private_key() -> Zeroizing<[u8; 32]> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(bytes.as_mut());
        bytes
    }

    /// Derives a public key from a private key.
    ///
    /// Returns a 96-byte compressed G2 point.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 96]> {
        use blst::blst_p2;

        let sk_scalar = Self::validate_private_key(private_key)?;

        // SAFETY: `sk_scalar` was validated by `validate_private_key`. All blst FFI
        // calls operate on stack-allocated, default-initialized structs with no aliasing.
        // `blst_p2_compress` writes exactly 96 bytes into `pk_bytes`.
        unsafe {
            let mut pk = blst_p2::default();
            blst::blst_sk_to_pk_in_g2(&mut pk, &sk_scalar);

            let mut pk_bytes = [0u8; 96];
            blst::blst_p2_compress(pk_bytes.as_mut_ptr(), &pk);

            Ok(pk_bytes)
        }
    }

    /// Signs a message with BLS12-381.
    ///
    /// Returns a 48-byte compressed G1 signature.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 48]> {
        use blst::blst_p1;

        let sk_scalar = Self::validate_private_key(private_key)?;

        // SAFETY: `sk_scalar` validated above. `message` and `NEO_BLS_DST` are valid
        // slices with correct lengths passed via `as_ptr()`/`len()`. All blst FFI calls
        // operate on stack-allocated, default-initialized structs. `blst_p1_compress`
        // writes exactly 48 bytes into `sig_bytes`.
        unsafe {
            let mut msg_point = blst_p1::default();
            blst::blst_hash_to_g1(
                &mut msg_point,
                message.as_ptr(),
                message.len(),
                NEO_BLS_DST.as_ptr(),
                NEO_BLS_DST.len(),
                std::ptr::null(),
                0,
            );

            let mut signature = blst_p1::default();
            blst::blst_p1_mult(&mut signature, &msg_point, sk_scalar.b.as_ptr(), 255);

            let mut sig_bytes = [0u8; 48];
            blst::blst_p1_compress(sig_bytes.as_mut_ptr(), &signature);

            Ok(sig_bytes)
        }
    }

    /// Verifies a BLS12-381 signature.
    ///
    /// Signature is a 48-byte compressed G1 point; public key is a 96-byte
    /// compressed G2 point.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 48],
        public_key: &[u8; 96],
    ) -> CryptoResult<bool> {
        use blst::{blst_p1_affine, blst_p2_affine, BLST_ERROR};

        // SAFETY: All inputs are fixed-size arrays with correct lengths for blst FFI.
        // Each deserialized point is validated (subgroup check, infinity check) before
        // use. Stack-allocated structs are default-initialized before FFI writes.
        unsafe {
            let mut sig_affine = blst_p1_affine::default();
            let result = blst::blst_p1_uncompress(&mut sig_affine, signature.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(CryptoError::invalid_signature("Invalid signature encoding"));
            }

            if blst::blst_p1_affine_is_inf(&sig_affine) || !blst::blst_p1_affine_in_g1(&sig_affine)
            {
                return Err(CryptoError::invalid_signature(
                    "Signature not in G1 subgroup",
                ));
            }

            let mut pk_affine = blst_p2_affine::default();
            let result = blst::blst_p2_uncompress(&mut pk_affine, public_key.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(CryptoError::invalid_key("Invalid public key encoding"));
            }

            if blst::blst_p2_affine_is_inf(&pk_affine) || !blst::blst_p2_affine_in_g2(&pk_affine) {
                return Err(CryptoError::invalid_key("Public key not in G2 subgroup"));
            }

            let result = blst::blst_core_verify_pk_in_g2(
                &pk_affine,
                &sig_affine,
                true,
                message.as_ptr(),
                message.len(),
                NEO_BLS_DST.as_ptr(),
                NEO_BLS_DST.len(),
                std::ptr::null(),
                0,
            );

            Ok(result == BLST_ERROR::BLST_SUCCESS)
        }
    }

    /// Aggregates multiple BLS signatures into one.
    ///
    /// Used for dBFT consensus where multiple validators sign.
    pub fn aggregate_signatures(signatures: &[[u8; 48]]) -> CryptoResult<[u8; 48]> {
        use blst::{blst_p1, blst_p1_affine};

        if signatures.is_empty() {
            return Err(CryptoError::invalid_argument("No signatures to aggregate"));
        }

        if signatures.len() == 1 {
            return Ok(signatures[0]);
        }

        // SAFETY: Each signature is a fixed 48-byte array. Every deserialized point
        // is validated (subgroup + infinity check) before aggregation. Stack-allocated
        // structs are default-initialized. Loop bounds match `signatures.len()`.
        unsafe {
            let mut agg = blst_p1::default();
            let mut first_affine = blst_p1_affine::default();
            let result = blst::blst_p1_uncompress(&mut first_affine, signatures[0].as_ptr());
            if result != blst::BLST_ERROR::BLST_SUCCESS {
                return Err(CryptoError::invalid_signature("Invalid first signature"));
            }
            if blst::blst_p1_affine_is_inf(&first_affine)
                || !blst::blst_p1_affine_in_g1(&first_affine)
            {
                return Err(CryptoError::invalid_signature(
                    "First signature not in G1 subgroup",
                ));
            }
            blst::blst_p1_from_affine(&mut agg, &first_affine);

            for sig in &signatures[1..] {
                let mut sig_affine = blst_p1_affine::default();
                let result = blst::blst_p1_uncompress(&mut sig_affine, sig.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_signature(
                        "Invalid signature in aggregation",
                    ));
                }
                if blst::blst_p1_affine_is_inf(&sig_affine)
                    || !blst::blst_p1_affine_in_g1(&sig_affine)
                {
                    return Err(CryptoError::invalid_signature(
                        "Signature not in G1 subgroup",
                    ));
                }
                blst::blst_p1_add_or_double_affine(&mut agg, &agg, &sig_affine);
            }

            let mut out = [0u8; 48];
            blst::blst_p1_compress(out.as_mut_ptr(), &agg);

            Ok(out)
        }
    }

    /// Verifies an aggregated signature against multiple public keys.
    pub fn verify_aggregated(
        message: &[u8],
        aggregated_signature: &[u8; 48],
        public_keys: &[[u8; 96]],
    ) -> CryptoResult<bool> {
        use blst::{blst_p2, blst_p2_affine};

        if public_keys.is_empty() {
            return Err(CryptoError::invalid_argument("No public keys provided"));
        }

        // SAFETY: Each public key is a fixed 96-byte array. Every deserialized G2 point
        // is validated (subgroup + infinity check) before aggregation. The aggregated
        // key is then used for pairing verification via the already-audited `verify`.
        unsafe {
            let mut agg_pk = blst_p2::default();
            let mut first_affine = blst_p2_affine::default();
            let result = blst::blst_p2_uncompress(&mut first_affine, public_keys[0].as_ptr());
            if result != blst::BLST_ERROR::BLST_SUCCESS {
                return Err(CryptoError::invalid_key("Invalid first public key"));
            }
            if blst::blst_p2_affine_is_inf(&first_affine)
                || !blst::blst_p2_affine_in_g2(&first_affine)
            {
                return Err(CryptoError::invalid_key(
                    "First public key not in G2 subgroup",
                ));
            }
            blst::blst_p2_from_affine(&mut agg_pk, &first_affine);

            for pk in &public_keys[1..] {
                let mut pk_affine = blst_p2_affine::default();
                let result = blst::blst_p2_uncompress(&mut pk_affine, pk.as_ptr());
                if result != blst::BLST_ERROR::BLST_SUCCESS {
                    return Err(CryptoError::invalid_key(
                        "Invalid public key in aggregation",
                    ));
                }
                if blst::blst_p2_affine_is_inf(&pk_affine)
                    || !blst::blst_p2_affine_in_g2(&pk_affine)
                {
                    return Err(CryptoError::invalid_key("Public key not in G2 subgroup"));
                }
                blst::blst_p2_add_or_double_affine(&mut agg_pk, &agg_pk, &pk_affine);
            }

            let mut agg_pk_bytes = [0u8; 96];
            blst::blst_p2_compress(agg_pk_bytes.as_mut_ptr(), &agg_pk);

            Self::verify(message, aggregated_signature, &agg_pk_bytes)
        }
    }
}

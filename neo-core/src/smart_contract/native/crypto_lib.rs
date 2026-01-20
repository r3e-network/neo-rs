//! CryptoLib native contract implementation.
//!
//! Provides cryptographic functions for the Neo blockchain.
//! Matches the C# Neo.SmartContract.Native.CryptoLib contract.

use crate::cryptography::crypto_utils::murmur::murmur32;
use crate::cryptography::{Crypto, Ed25519Crypto, HashAlgorithm, NamedCurveHash};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::ContractParameterType;
use crate::UInt160;
use neo_vm::{ExecutionEngineLimits, StackItem};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;

// BLS12-381 support using blst crate
use blst::{
    blst_fp, blst_fp12, blst_p1, blst_p1_affine, blst_p2, blst_p2_affine, blst_scalar, BLST_ERROR,
};

pub struct CryptoLib {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl CryptoLib {
    const ID: i32 = -3;

    pub fn new() -> Self {
        // CryptoLib contract hash: 0x726cb6e0cd8628a1350a611384688911ab75f51b
        let hash = UInt160::parse("0x726cb6e0cd8628a1350a611384688911ab75f51b")
            .expect("Valid CryptoLib contract hash");

        let methods = vec![
            // Hash functions
            NativeMethod::safe(
                "recoverSecp256K1".to_string(),
                1 << 15,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::ByteArray,
            )
            .with_active_in(Hardfork::HfEchidna),
            NativeMethod::safe(
                "sha256".to_string(),
                1 << 15,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "ripemd160".to_string(),
                1 << 15,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "murmur32".to_string(),
                1 << 13,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "keccak256".to_string(),
                1 << 15,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::ByteArray,
            )
            .with_active_in(Hardfork::HfCockatrice),
            // ECDSA functions
            NativeMethod::safe(
                "verifyWithECDsa".to_string(),
                1 << 15,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfCockatrice),
            NativeMethod::safe(
                "verifyWithECDsa".to_string(),
                1 << 15,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfCockatrice),
            NativeMethod::safe(
                "verifyWithEd25519".to_string(),
                1 << 15,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfEchidna),
            // BLS12-381 functions
            NativeMethod::safe(
                "bls12381Add".to_string(),
                1 << 19,
                vec![
                    ContractParameterType::InteropInterface,
                    ContractParameterType::InteropInterface,
                ],
                ContractParameterType::InteropInterface,
            ),
            NativeMethod::safe(
                "bls12381Equal".to_string(),
                1 << 5,
                vec![
                    ContractParameterType::InteropInterface,
                    ContractParameterType::InteropInterface,
                ],
                ContractParameterType::Boolean,
            ),
            NativeMethod::safe(
                "bls12381Mul".to_string(),
                1 << 21,
                vec![
                    ContractParameterType::InteropInterface,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::InteropInterface,
            ),
            NativeMethod::safe(
                "bls12381Pairing".to_string(),
                1 << 23,
                vec![
                    ContractParameterType::InteropInterface,
                    ContractParameterType::InteropInterface,
                ],
                ContractParameterType::InteropInterface,
            ),
            NativeMethod::safe(
                "bls12381Serialize".to_string(),
                1 << 19,
                vec![ContractParameterType::InteropInterface],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "bls12381Deserialize".to_string(),
                1 << 19,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::InteropInterface,
            ),
        ];
        let methods = methods
            .into_iter()
            .map(
                |method| match (method.name.as_str(), method.parameters.len()) {
                    ("bls12381Add", 2) => {
                        method.with_parameter_names(vec!["x".to_string(), "y".to_string()])
                    }
                    ("bls12381Deserialize", 1) => {
                        method.with_parameter_names(vec!["data".to_string()])
                    }
                    ("bls12381Equal", 2) => {
                        method.with_parameter_names(vec!["x".to_string(), "y".to_string()])
                    }
                    ("bls12381Mul", 3) => method.with_parameter_names(vec![
                        "x".to_string(),
                        "mul".to_string(),
                        "neg".to_string(),
                    ]),
                    ("bls12381Pairing", 2) => {
                        method.with_parameter_names(vec!["g1".to_string(), "g2".to_string()])
                    }
                    ("bls12381Serialize", 1) => method.with_parameter_names(vec!["g".to_string()]),
                    ("keccak256", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("murmur32", 2) => {
                        method.with_parameter_names(vec!["data".to_string(), "seed".to_string()])
                    }
                    ("recoverSecp256K1", 2) => method.with_parameter_names(vec![
                        "messageHash".to_string(),
                        "signature".to_string(),
                    ]),
                    ("ripemd160", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("sha256", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("verifyWithECDsa", 4) => method.with_parameter_names(vec![
                        "message".to_string(),
                        "pubkey".to_string(),
                        "signature".to_string(),
                        "curveHash".to_string(),
                    ]),
                    ("verifyWithEd25519", 3) => method.with_parameter_names(vec![
                        "message".to_string(),
                        "pubkey".to_string(),
                        "signature".to_string(),
                    ]),
                    _ => method,
                },
            )
            .collect();

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// SHA256 hash function backed by the shared cryptography crate.
    fn sha256(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args
            .first()
            .ok_or_else(|| Error::native_contract("sha256 requires data argument".to_string()))?;

        Ok(Crypto::sha256(data).to_vec())
    }

    /// RIPEMD160 hash function backed by the shared cryptography crate.
    fn ripemd160(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args.first().ok_or_else(|| {
            Error::native_contract("ripemd160 requires data argument".to_string())
        })?;

        Ok(Crypto::ripemd160(data).to_vec())
    }

    /// Murmur32 hash function backed by the shared cryptography crate.
    fn murmur32(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "murmur32 requires data and seed arguments".to_string(),
            ));
        }

        let seed = BigInt::from_signed_bytes_le(&args[1])
            .to_u32()
            .ok_or_else(|| Error::invalid_argument("Invalid murmur32 seed".to_string()))?;
        let hash = murmur32(&args[0], seed);
        Ok(hash.to_le_bytes().to_vec())
    }

    /// Keccak256 hash function backed by the shared cryptography crate.
    fn keccak256(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args.first().ok_or_else(|| {
            Error::native_contract("keccak256 requires data argument".to_string())
        })?;

        Ok(Crypto::keccak256(data).to_vec())
    }

    fn wrap_interop_bytes(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        BinarySerializer::serialize(
            &StackItem::from_byte_string(data),
            &ExecutionEngineLimits::default(),
        )
        .map_err(|e| Error::serialization(format!("Failed to serialize interop payload: {e}")))
    }

    /// Verify ECDSA signature with named curve/hash pair.
    fn verify_with_ecdsa(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 4 {
            return Err(Error::native_contract(
                "verifyWithECDsa requires message, public key, signature, and curveHash arguments"
                    .to_string(),
            ));
        }

        let message = &args[0];
        let public_key = &args[1];
        let signature = &args[2];
        let curve_arg = &args[3];

        let cockatrice_enabled = engine.is_hardfork_enabled(Hardfork::HfCockatrice);
        let curve_hash = match Self::parse_named_curve_hash(curve_arg) {
            Some(value) => value,
            None if cockatrice_enabled => return Ok(vec![0]),
            None => {
                return Err(Error::invalid_argument(
                    "Invalid curve hash for verifyWithECDsa".to_string(),
                ))
            }
        };

        if !cockatrice_enabled
            && !matches!(
                curve_hash,
                NamedCurveHash::Secp256k1SHA256 | NamedCurveHash::Secp256r1SHA256
            )
        {
            return Err(Error::invalid_argument(
                "Unsupported curve hash for legacy verifyWithECDsa".to_string(),
            ));
        }

        if signature.len() != 64 || public_key.is_empty() {
            return Ok(vec![0]);
        }

        let curve = curve_hash.curve();
        let hash_algorithm = if cockatrice_enabled {
            curve_hash.hash_algorithm()
        } else {
            HashAlgorithm::Sha256
        };

        let is_valid = Crypto::verify_signature_with_curve(
            message,
            signature,
            public_key,
            &curve,
            hash_algorithm,
        );

        Ok(vec![if is_valid { 1 } else { 0 }])
    }

    /// Verify Ed25519 signature (Echidna+).
    fn verify_with_ed25519(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 3 {
            return Err(Error::native_contract(
                "verifyWithEd25519 requires message, public key, and signature arguments"
                    .to_string(),
            ));
        }

        let message = &args[0];
        let public_key = &args[1];
        let signature = &args[2];

        if signature.len() != 64 || public_key.len() != 32 {
            return Ok(vec![0]);
        }

        let sig_bytes: [u8; 64] = match signature.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => return Ok(vec![0]),
        };
        let pub_bytes: [u8; 32] = match public_key.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => return Ok(vec![0]),
        };

        let is_valid = Ed25519Crypto::verify(message, &sig_bytes, &pub_bytes).unwrap_or(false);
        Ok(vec![if is_valid { 1 } else { 0 }])
    }

    /// Recover a secp256k1 public key from a message hash and signature.
    fn recover_secp256k1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "recoverSecp256K1 requires messageHash and signature arguments".to_string(),
            ));
        }

        let message_hash = &args[0];
        let signature = &args[1];

        match crate::cryptography::Secp256k1Crypto::recover_public_key(message_hash, signature) {
            Ok(public_key) => Ok(public_key),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn parse_named_curve_hash(arg: &[u8]) -> Option<NamedCurveHash> {
        BigInt::from_signed_bytes_le(arg)
            .to_u8()
            .and_then(NamedCurveHash::from_byte)
    }

    /// BLS12-381 point addition
    fn bls12381_add(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Add requires two point arguments".to_string(),
            ));
        }

        let x = &args[0];
        let y = &args[1];

        let bytes = match x.len() {
            48 => {
                if y.len() != 48 {
                    return Err(Error::native_contract("Point size mismatch".to_string()));
                }
                let p1 = self.deserialize_g1(x)?;
                let p2 = self.deserialize_g1(y)?;
                let result = self.g1_add(&p1, &p2);
                self.serialize_g1(&result)?
            }
            96 => {
                if y.len() != 96 {
                    return Err(Error::native_contract("Point size mismatch".to_string()));
                }
                let p1 = self.deserialize_g2(x)?;
                let p2 = self.deserialize_g2(y)?;
                let result = self.g2_add(&p1, &p2);
                self.serialize_g2(&result)?
            }
            576 => {
                if y.len() != 576 {
                    return Err(Error::native_contract("Point size mismatch".to_string()));
                }
                let p1 = self.deserialize_gt(x)?;
                let p2 = self.deserialize_gt(y)?;
                let result = self.gt_add(&p1, &p2);
                self.serialize_gt(&result)?
            }
            _ => {
                return Err(Error::native_contract(
                    "Invalid BLS12-381 point size".to_string(),
                ))
            }
        };

        self.wrap_interop_bytes(bytes)
    }

    /// BLS12-381 equality check
    fn bls12381_equal(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Equal requires two point arguments".to_string(),
            ));
        }

        let x = &args[0];
        let y = &args[1];

        match x.len() {
            48 => {
                if y.len() != 48 {
                    return Err(Error::native_contract(
                        "BLS12-381 type mismatch".to_string(),
                    ));
                }
                let p1 = self.deserialize_g1(x)?;
                let p2 = self.deserialize_g1(y)?;
                let equal = unsafe { blst::blst_p1_affine_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
            96 => {
                if y.len() != 96 {
                    return Err(Error::native_contract(
                        "BLS12-381 type mismatch".to_string(),
                    ));
                }
                let p1 = self.deserialize_g2(x)?;
                let p2 = self.deserialize_g2(y)?;
                let equal = unsafe { blst::blst_p2_affine_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
            576 => {
                if y.len() != 576 {
                    return Err(Error::native_contract(
                        "BLS12-381 type mismatch".to_string(),
                    ));
                }
                let p1 = self.deserialize_gt(x)?;
                let p2 = self.deserialize_gt(y)?;
                let equal = unsafe { blst::blst_fp12_is_equal(&p1, &p2) };
                Ok(vec![if equal { 1 } else { 0 }])
            }
            _ => Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            )),
        }
    }

    /// BLS12-381 scalar multiplication
    fn bls12381_mul(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Mul requires point and scalar arguments".to_string(),
            ));
        }

        let point = &args[0];
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

        let bytes = match point.len() {
            48 => {
                let p = self.deserialize_g1(point)?;
                let result = self.g1_mul(&p, &scalar_bytes, neg);
                self.serialize_g1(&result)?
            }
            96 => {
                let p = self.deserialize_g2(point)?;
                let result = self.g2_mul(&p, &scalar_bytes, neg);
                self.serialize_g2(&result)?
            }
            576 => {
                let p = self.deserialize_gt(point)?;
                let result = self.gt_mul(&p, &scalar_bytes, neg);
                self.serialize_gt(&result)?
            }
            _ => {
                return Err(Error::native_contract(
                    "Invalid BLS12-381 point size".to_string(),
                ))
            }
        };

        self.wrap_interop_bytes(bytes)
    }

    /// BLS12-381 pairing operation
    fn bls12381_pairing(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "bls12381Pairing requires G1 and G2 point arguments".to_string(),
            ));
        }

        let g1_point = &args[0];
        let g2_point = &args[1];

        if g1_point.len() != 48 || g2_point.len() != 96 {
            return Err(Error::native_contract(
                "Invalid point sizes for pairing (G1: 48, G2: 96 bytes)".to_string(),
            ));
        }

        let p1 = self.deserialize_g1(g1_point)?;
        let p2 = self.deserialize_g2(g2_point)?;

        let bytes = self.compute_pairing(&p1, &p2)?;
        self.wrap_interop_bytes(bytes)
    }

    /// BLS12-381 point serialization
    fn bls12381_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Serialize requires point argument".to_string(),
            ));
        }
        let data = &args[0];
        match data.len() {
            48 => {
                let point = self.deserialize_g1(data)?;
                let mut proj = blst_p1::default();
                unsafe {
                    blst::blst_p1_from_affine(&mut proj, &point);
                }
                self.serialize_g1(&proj)
            }
            96 => {
                let point = self.deserialize_g2(data)?;
                let mut proj = blst_p2::default();
                unsafe {
                    blst::blst_p2_from_affine(&mut proj, &point);
                }
                self.serialize_g2(&proj)
            }
            576 => {
                let point = self.deserialize_gt(data)?;
                self.serialize_gt(&point)
            }
            _ => Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            )),
        }
    }

    /// BLS12-381 point deserialization
    fn bls12381_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Deserialize requires bytes argument".to_string(),
            ));
        }

        let data = &args[0];

        let bytes = match data.len() {
            48 => {
                let _ = self.deserialize_g1(data)?;
                data.clone()
            }
            96 => {
                let _ = self.deserialize_g2(data)?;
                data.clone()
            }
            576 => {
                let _ = self.deserialize_gt(data)?;
                data.clone()
            }
            _ => {
                return Err(Error::native_contract(
                    "Invalid BLS12-381 serialized point size".to_string(),
                ))
            }
        };

        self.wrap_interop_bytes(bytes)
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
                return Err(Error::native_contract("Invalid G1 point".to_string()));
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
                return Err(Error::native_contract("Invalid G2 point".to_string()));
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
        let mut result = unsafe { *blst::blst_fp12_one() };
        let base = *p;

        for byte in scalar.iter().rev() {
            for bit in (0..8).rev() {
                // SAFETY: result and base are valid blst_fp12 values.
                unsafe {
                    blst::blst_fp12_sqr(&mut result, &result);
                }
                if (byte >> bit) & 1 == 1 {
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

        unsafe {
            blst::blst_fp_from_bendian(target, data.as_ptr());
        }

        let mut check = [0u8; FP_SIZE];
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

impl NativeContract for CryptoLib {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "CryptoLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "recoverSecp256K1" => self.recover_secp256k1(args),
            "sha256" => self.sha256(args),
            "ripemd160" => self.ripemd160(args),
            "murmur32" => self.murmur32(args),
            "keccak256" => self.keccak256(args),
            "verifyWithECDsa" => self.verify_with_ecdsa(engine, args),
            "verifyWithEd25519" => self.verify_with_ed25519(args),
            "bls12381Add" => self.bls12381_add(args),
            "bls12381Equal" => self.bls12381_equal(args),
            "bls12381Mul" => self.bls12381_mul(args),
            "bls12381Pairing" => self.bls12381_pairing(args),
            "bls12381Serialize" => self.bls12381_serialize(args),
            "bls12381Deserialize" => self.bls12381_deserialize(args),
            _ => Err(Error::native_contract(format!(
                "Unknown CryptoLib method: {}",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for CryptoLib {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let lib = CryptoLib::new();
        let data = b"hello world".to_vec();
        let result = lib.sha256(&[data]).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_ripemd160() {
        let lib = CryptoLib::new();
        let data = b"hello world".to_vec();
        let result = lib.ripemd160(&[data]).unwrap();
        assert_eq!(result.len(), 20);
    }
}

//! CryptoLib native contract implementation.
//!
//! Provides cryptographic functions for the Neo blockchain.
//! Matches the C# Neo.SmartContract.Native.CryptoLib contract.

use crate::UInt160;
use crate::cryptography::crypto_utils::murmur::murmur32;
use crate::cryptography::{Crypto, Ed25519Crypto, HashAlgorithm, NamedCurveHash};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeMethod;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

// BLS12-381 support using blst crate
use blst::{blst_p1, blst_p2};

mod bls_interop;
pub(crate) use bls_interop::{Bls12381Group, Bls12381Interop, Bls12381Kind};
mod bls_helpers;

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
            .ok_or_else(|| Error::native_contract("sha256 requires data argument"))?;

        Ok(Crypto::sha256(data).to_vec())
    }

    /// RIPEMD160 hash function backed by the shared cryptography crate.
    fn ripemd160(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = args
            .first()
            .ok_or_else(|| Error::native_contract("ripemd160 requires data argument"))?;

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
        let data = args
            .first()
            .ok_or_else(|| Error::native_contract("keccak256 requires data argument"))?;

        Ok(Crypto::keccak256(data).to_vec())
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
                ));
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
    fn bls12381_equal(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
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
    fn bls12381_mul(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
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
    fn bls12381_pairing(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
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
    fn bls12381_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
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
    fn bls12381_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
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
                ));
            }
        };

        Ok(Bls12381Interop::new(kind, bytes)?.to_encoded_bytes())
    }
}

mod native_impl;

#[cfg(test)]
mod tests;

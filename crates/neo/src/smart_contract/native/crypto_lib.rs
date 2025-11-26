//! CryptoLib native contract implementation.
//!
//! Provides cryptographic functions for the Neo blockchain.
//! Matches the C# Neo.SmartContract.Native.CryptoLib contract.

use crate::cryptography::crypto_utils::ECCurve;
use crate::cryptography::{Crypto, HashAlgorithm};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::UInt160;
use std::any::Any;

// BLS12-381 support using blst crate
use blst::{blst_fp12, blst_p1, blst_p1_affine, blst_p2, blst_p2_affine, blst_scalar, BLST_ERROR};

pub struct CryptoLib {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl CryptoLib {
    const ID: i32 = -3;

    pub fn new() -> Self {
        // CryptoLib contract hash: 0x726cb6e0cd8628a1350a611384688911ab75f51b
        let hash = UInt160::from_bytes(&[
            0x72, 0x6c, 0xb6, 0xe0, 0xcd, 0x86, 0x28, 0xa1, 0x35, 0x0a, 0x61, 0x13, 0x84, 0x68,
            0x89, 0x11, 0xab, 0x75, 0xf5, 0x1b,
        ])
        .expect("Valid CryptoLib contract hash");

        let methods = vec![
            // Hash functions
            NativeMethod::safe("sha256".to_string(), 1 << 15),
            NativeMethod::safe("ripemd160".to_string(), 1 << 15),
            // ECDSA functions
            NativeMethod::safe("verifyWithECDsa".to_string(), 1 << 15),
            NativeMethod::safe("verifyWithECDsaSecp256k1".to_string(), 1 << 15),
            NativeMethod::safe("verifyWithECDsaSecp256r1".to_string(), 1 << 15),
            // Multi-signature verification
            NativeMethod::safe("checkMultisig".to_string(), 1 << 16),
            NativeMethod::safe("checkMultisigWithECDsaSecp256k1".to_string(), 1 << 16),
            NativeMethod::safe("checkMultisigWithECDsaSecp256r1".to_string(), 1 << 16),
            // BLS12-381 functions
            NativeMethod::safe("bls12381Add".to_string(), 1 << 19),
            NativeMethod::safe("bls12381Equal".to_string(), 1 << 5),
            NativeMethod::safe("bls12381Mul".to_string(), 1 << 19),
            NativeMethod::safe("bls12381Pairing".to_string(), 1 << 20),
            NativeMethod::safe("bls12381Serialize".to_string(), 1 << 16),
            NativeMethod::safe("bls12381Deserialize".to_string(), 1 << 16),
        ];

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

    /// Verify ECDSA signature (default secp256r1)
    fn verify_with_ecdsa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256r1(),
            "verifyWithECDsa requires message, signature, and public key arguments",
        )
    }

    /// Verify ECDSA signature with secp256k1 curve
    fn verify_with_ecdsa_secp256k1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256k1(),
            "verifyWithECDsaSecp256k1 requires message, signature, and public key arguments",
        )
    }

    /// Verify ECDSA signature with secp256r1 curve (Neo's default)
    fn verify_with_ecdsa_secp256r1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify_with_curve(
            args,
            ECCurve::secp256r1(),
            "verifyWithECDsaSecp256r1 requires message, signature, and public key arguments",
        )
    }

    fn verify_with_curve(
        &self,
        args: &[Vec<u8>],
        curve: ECCurve,
        error_msg: &str,
    ) -> Result<Vec<u8>> {
        if args.len() < 3 {
            return Err(Error::native_contract(error_msg.to_string()));
        }

        let message = &args[0];
        let signature = &args[1];
        let public_key = &args[2];

        if signature.len() != 64 || public_key.is_empty() {
            return Ok(vec![0]);
        }

        let is_valid = Crypto::verify_signature_with_curve(
            message,
            signature,
            public_key,
            &curve,
            HashAlgorithm::Sha256,
        );

        Ok(vec![if is_valid { 1 } else { 0 }])
    }

    /// Check multi-signature (default secp256r1)
    fn check_multisig(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256r1(),
            "checkMultisig requires message, signatures, and public keys arguments",
        )
    }

    /// Check multi-signature with secp256k1 curve
    fn check_multisig_secp256k1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256k1(),
            "checkMultisigWithECDsaSecp256k1 requires message, signatures, and public keys arguments",
        )
    }

    /// Check multi-signature with secp256r1 curve (Neo's default)
    fn check_multisig_secp256r1(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.check_multisig_with_curve(
            args,
            ECCurve::secp256r1(),
            "checkMultisigWithECDsaSecp256r1 requires message, signatures, and public keys arguments",
        )
    }

    fn check_multisig_with_curve(
        &self,
        args: &[Vec<u8>],
        curve: ECCurve,
        error_msg: &str,
    ) -> Result<Vec<u8>> {
        if args.len() < 3 {
            return Err(Error::native_contract(error_msg.to_string()));
        }

        let message = &args[0];
        let signatures_data = &args[1];
        let public_keys_data = &args[2];

        let signatures = self.parse_signature_array(signatures_data)?;
        let public_keys = self.parse_public_key_array(public_keys_data)?;

        if signatures.len() > public_keys.len() {
            return Ok(vec![0]);
        }

        let mut sig_index = 0;
        for pubkey in &public_keys {
            if sig_index >= signatures.len() {
                break;
            }

            if Crypto::verify_signature_with_curve(
                message,
                &signatures[sig_index],
                pubkey,
                &curve,
                HashAlgorithm::Sha256,
            ) {
                sig_index += 1;
            }
        }

        Ok(vec![if sig_index == signatures.len() { 1 } else { 0 }])
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

        match x.len() {
            48 => {
                if y.len() != 48 {
                    return Err(Error::native_contract("Point size mismatch".to_string()));
                }
                let p1 = self.deserialize_g1(x)?;
                let p2 = self.deserialize_g1(y)?;
                let result = self.g1_add(&p1, &p2);
                self.serialize_g1(&result)
            }
            96 => {
                if y.len() != 96 {
                    return Err(Error::native_contract("Point size mismatch".to_string()));
                }
                let p1 = self.deserialize_g2(x)?;
                let p2 = self.deserialize_g2(y)?;
                let result = self.g2_add(&p1, &p2);
                self.serialize_g2(&result)
            }
            _ => Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            )),
        }
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

        if x.len() != y.len() {
            return Ok(vec![0]);
        }

        let equal = x == y;
        Ok(vec![if equal { 1 } else { 0 }])
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

        let mut scalar_bytes = [0u8; 32];
        let len = scalar.len().min(32);
        scalar_bytes[..len].copy_from_slice(&scalar[..len]);

        match point.len() {
            48 => {
                let p = self.deserialize_g1(point)?;
                let result = self.g1_mul(&p, &scalar_bytes, neg);
                self.serialize_g1(&result)
            }
            96 => {
                let p = self.deserialize_g2(point)?;
                let result = self.g2_mul(&p, &scalar_bytes, neg);
                self.serialize_g2(&result)
            }
            _ => Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            )),
        }
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

        self.compute_pairing(&p1, &p2)
    }

    /// BLS12-381 point serialization
    fn bls12381_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Serialize requires point argument".to_string(),
            ));
        }
        Ok(args[0].clone())
    }

    /// BLS12-381 point deserialization
    fn bls12381_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "bls12381Deserialize requires bytes argument".to_string(),
            ));
        }

        let data = &args[0];

        match data.len() {
            48 => {
                let _ = self.deserialize_g1(data)?;
                Ok(data.clone())
            }
            96 => {
                let _ = self.deserialize_g2(data)?;
                Ok(data.clone())
            }
            576 => Ok(data.clone()),
            _ => Err(Error::native_contract(
                "Invalid BLS12-381 serialized point size".to_string(),
            )),
        }
    }

    // BLS12-381 helper functions

    fn deserialize_g1(&self, data: &[u8]) -> Result<blst_p1_affine> {
        let mut point = blst_p1_affine::default();
        unsafe {
            let result = blst::blst_p1_uncompress(&mut point, data.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(Error::native_contract("Invalid G1 point".to_string()));
            }
        }
        Ok(point)
    }

    fn deserialize_g2(&self, data: &[u8]) -> Result<blst_p2_affine> {
        let mut point = blst_p2_affine::default();
        unsafe {
            let result = blst::blst_p2_uncompress(&mut point, data.as_ptr());
            if result != BLST_ERROR::BLST_SUCCESS {
                return Err(Error::native_contract("Invalid G2 point".to_string()));
            }
        }
        Ok(point)
    }

    fn serialize_g1(&self, point: &blst_p1) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 48];
        unsafe {
            blst::blst_p1_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    fn serialize_g2(&self, point: &blst_p2) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 96];
        unsafe {
            blst::blst_p2_compress(out.as_mut_ptr(), point);
        }
        Ok(out)
    }

    fn g1_add(&self, p1: &blst_p1_affine, p2: &blst_p1_affine) -> blst_p1 {
        let mut result = blst_p1::default();
        let mut p1_proj = blst_p1::default();
        let mut p2_proj = blst_p1::default();
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
        unsafe {
            blst::blst_p2_from_affine(&mut p1_proj, p1);
            blst::blst_p2_from_affine(&mut p2_proj, p2);
            blst::blst_p2_add(&mut result, &p1_proj, &p2_proj);
        }
        result
    }

    fn g1_mul(&self, p: &blst_p1_affine, scalar: &[u8; 32], neg: bool) -> blst_p1 {
        let mut result = blst_p1::default();
        let mut p_proj = blst_p1::default();
        let mut scalar_val = blst_scalar::default();

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

    fn compute_pairing(&self, g1: &blst_p1_affine, g2: &blst_p2_affine) -> Result<Vec<u8>> {
        let mut result = blst_fp12::default();
        unsafe {
            blst::blst_miller_loop(&mut result, g2, g1);
            blst::blst_final_exp(&mut result, &result);
        }

        let mut out = vec![0u8; 576];
        unsafe {
            std::ptr::copy_nonoverlapping(&result as *const _ as *const u8, out.as_mut_ptr(), 576);
        }
        Ok(out)
    }

    fn parse_signature_array(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Ok(vec![]);
        }

        let mut signatures = Vec::new();
        let mut offset = 0;

        while offset + 64 <= data.len() {
            signatures.push(data[offset..offset + 64].to_vec());
            offset += 64;
        }

        Ok(signatures)
    }

    fn parse_public_key_array(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Ok(vec![]);
        }

        let mut public_keys = Vec::new();
        let mut offset = 0;

        while offset + 33 <= data.len() {
            public_keys.push(data[offset..offset + 33].to_vec());
            offset += 33;
        }

        Ok(public_keys)
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
        _engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "sha256" => self.sha256(args),
            "ripemd160" => self.ripemd160(args),
            "verifyWithECDsa" => self.verify_with_ecdsa(args),
            "verifyWithECDsaSecp256k1" => self.verify_with_ecdsa_secp256k1(args),
            "verifyWithECDsaSecp256r1" => self.verify_with_ecdsa_secp256r1(args),
            "checkMultisig" => self.check_multisig(args),
            "checkMultisigWithECDsaSecp256k1" => self.check_multisig_secp256k1(args),
            "checkMultisigWithECDsaSecp256r1" => self.check_multisig_secp256r1(args),
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

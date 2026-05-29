//! CryptoLib native contract implementation.
//!
//! Provides cryptographic functions for the Neo blockchain.
//! Matches the C# Neo.SmartContract.Native.CryptoLib contract.

use neo_crypto::crypto_utils::murmur::murmur32;
use neo_crypto::{Crypto, Ed25519Crypto, HashAlgorithm, NamedCurveHash};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::UInt160;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;

mod bls12381;
mod metadata;
pub(crate) use bls12381::Bls12381Interop;

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

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
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

        match neo_crypto::Secp256k1Crypto::recover_public_key(message_hash, signature) {
            Ok(public_key) => Ok(public_key),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn parse_named_curve_hash(arg: &[u8]) -> Option<NamedCurveHash> {
        BigInt::from_signed_bytes_le(arg)
            .to_u8()
            .and_then(NamedCurveHash::from_byte)
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
        self.dispatch_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

crate::impl_default_via_new!(CryptoLib);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::trigger_type::TriggerType;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn test_engine() -> ApplicationEngine {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            400_000_000,
            None,
        )
        .expect("engine")
    }

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

    #[test]
    fn dispatch_method_routes_crypto_methods_and_unknowns() {
        let lib = CryptoLib::new();
        let mut engine = test_engine();
        let data = b"hello world".to_vec();

        assert_eq!(
            lib.dispatch_method(&mut engine, "sha256", std::slice::from_ref(&data))
                .expect("dispatch sha256"),
            lib.sha256(&[data]).expect("direct sha256")
        );

        let err = lib
            .dispatch_method(&mut engine, "missing", &[])
            .expect_err("unknown method");
        assert!(
            err.to_string()
                .contains("Unknown CryptoLib method: missing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dispatch_method_covers_declared_metadata_names() {
        let lib = CryptoLib::new();
        let mut engine = test_engine();
        let mut names = BTreeSet::new();

        for method in lib.methods() {
            if !names.insert(method.name.clone()) {
                continue;
            }

            if let Err(err) = lib.dispatch_method(&mut engine, &method.name, &[]) {
                assert!(
                    !err.to_string().contains("Unknown CryptoLib method"),
                    "declared method {} did not dispatch: {err}",
                    method.name
                );
            }
        }
    }
}

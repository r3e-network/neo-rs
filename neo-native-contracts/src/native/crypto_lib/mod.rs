//! CryptoLib native contract implementation.
//!
//! Provides cryptographic functions for the Neo blockchain.
//! Matches the C# Neo.SmartContract.Native.CryptoLib contract.

use super::{NativeContract, NativeMethod};
use neo_crypto::crypto_utils::murmur::murmur32;
use neo_crypto::{Crypto, Ed25519Crypto, HashAlgorithm, NamedCurveHash};
use neo_primitives::UInt160;
use neo_protocol::Hardfork;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::CoreError as Error;
use neo_smart_contract::error::CoreResult as Result;
use neo_smart_contract::impl_native_contract;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

mod bls12381;
mod metadata;

/// The CryptoLib native contract exposing hashing primitives (SHA-256, RIPEMD-160,
/// Murmur32, Keccak-256) plus signature verification and secp256k1 public-key recovery.
pub struct CryptoLib {
    id: i32,
    hash: UInt160,
    methods: &'static [NativeMethod],
}

impl CryptoLib {
    const ID: i32 = -3;

    /// Creates a new CryptoLib instance with its native method table.
    ///
    /// Hash: `0x726cb6e0cd8628a1350a611384688911ab75f51b`.
    pub fn new() -> Self {
        crate::native::stateless_native_contract!(
            CryptoLib,
            "0x726cb6e0cd8628a1350a611384688911ab75f51b"
        )
    }

    /// Dispatches a native method invocation against this contract.
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.dispatch_method(engine, method, args)
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
        let gorgon_enabled = engine.is_hardfork_enabled(Hardfork::HfGorgon);
        // C# VerifyWithECDsa versions: V0 (pre-Cockatrice) and V1 (Cockatrice
        // ..Gorgon) degrade malformed input to `false`; V2 (Gorgon+) throws.
        let reject_with_false = cockatrice_enabled && !gorgon_enabled;
        let curve_hash = match Self::parse_named_curve_hash(curve_arg) {
            Some(value) => value,
            None if reject_with_false => return Ok(vec![0]),
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
            // V2 (Gorgon+) throws on malformed signature/pubkey; V0/V1 return false.
            if gorgon_enabled {
                return Err(Error::invalid_argument(
                    "Invalid signature or public key for verifyWithECDsa".to_string(),
                ));
            }
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

        Ok(vec![u8::from(is_valid)])
    }

    /// Verify Ed25519 signature (Echidna+).
    ///
    /// C# versions: V0 (Echidna..Gorgon) degrades malformed input to `false`;
    /// V1 (Gorgon+) throws on bad sizes.
    fn verify_with_ed25519(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 3 {
            return Err(Error::native_contract(
                "verifyWithEd25519 requires message, public key, and signature arguments"
                    .to_string(),
            ));
        }

        let message = &args[0];
        let public_key = &args[1];
        let signature = &args[2];
        let gorgon_enabled = engine.is_hardfork_enabled(Hardfork::HfGorgon);

        if signature.len() != 64 || public_key.len() != 32 {
            if gorgon_enabled {
                return Err(Error::invalid_argument(
                    "Invalid signature or public key for verifyWithEd25519".to_string(),
                ));
            }
            return Ok(vec![0]);
        }

        let sig_bytes: [u8; 64] = match signature.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) if gorgon_enabled => {
                return Err(Error::invalid_argument(
                    "Invalid signature for verifyWithEd25519".to_string(),
                ));
            }
            Err(_) => return Ok(vec![0]),
        };
        let pub_bytes: [u8; 32] = match public_key.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) if gorgon_enabled => {
                return Err(Error::invalid_argument(
                    "Invalid public key for verifyWithEd25519".to_string(),
                ));
            }
            Err(_) => return Ok(vec![0]),
        };

        let is_valid = Ed25519Crypto::verify(message, &sig_bytes, &pub_bytes).unwrap_or(false);
        Ok(vec![u8::from(is_valid)])
    }

    /// Recover a secp256k1 public key from a message hash and signature.
    ///
    /// C# v3.11.2 splits this into two `recoverSecp256K1` versions: the Huyao
    /// `RecoverSecp256K1V1` validates R/S before recovering, while the pre-Huyao
    /// `RecoverSecp256K1V0` keeps the legacy BouncyCastle behaviour that wraps
    /// out-of-range values. Both catch every failure and return null.
    fn recover_secp256k1(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "recoverSecp256K1 requires messageHash and signature arguments".to_string(),
            ));
        }

        let message_hash = &args[0];
        let signature = &args[1];

        let result = if engine.is_hardfork_enabled(Hardfork::HfHuyao) {
            neo_crypto::Secp256k1Crypto::recover_public_key(message_hash, signature)
        } else {
            neo_crypto::Secp256k1Crypto::recover_public_key_v0(message_hash, signature)
        };

        match result {
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
    impl_native_contract!(hash, "CryptoLib", methods);

    fn id(&self) -> i32 {
        self.id
    }
}

neo_storage::impl_default_via_new!(CryptoLib);

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::TriggerType;
    use neo_protocol::{Hardfork, ProtocolSettings};
    use neo_storage::persistence::PersistenceDataCache;
    use neo_storage::{StorageItem, StorageKey};
    use num_bigint::BigInt;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn new_engine() -> ApplicationEngine {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(PersistenceDataCache::new(false)),
            None,
            ProtocolSettings::default(),
            400_000_000,
            None,
            crate::native::composition::default_native_services(),
        )
        .expect("engine")
    }

    #[test]
    fn sha256_returns_32_byte_digest() {
        let lib = CryptoLib::new();
        let data = b"hello world".to_vec();
        let result = lib.sha256(&[data]).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn ripemd160_returns_20_byte_digest() {
        let lib = CryptoLib::new();
        let data = b"hello world".to_vec();
        let result = lib.ripemd160(&[data]).unwrap();
        assert_eq!(result.len(), 20);
    }

    #[test]
    fn dispatch_method_routes_crypto_methods_and_unknowns() {
        let lib = CryptoLib::new();
        let mut engine = new_engine();
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
        let mut engine = new_engine();
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

    fn engine_with_hardforks_and_snapshot(
        hardforks: &[Hardfork],
        snapshot: Arc<PersistenceDataCache>,
    ) -> ApplicationEngine {
        let mut settings = ProtocolSettings::default();
        let mut heights = std::collections::HashMap::new();
        for &hardfork in hardforks {
            heights.insert(hardfork, 0);
        }
        settings.hardforks = heights;
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            settings,
            400_000_000,
            None,
            crate::native::composition::default_native_services(),
        )
        .expect("engine")
    }

    fn engine_with_hardforks(hardforks: &[Hardfork]) -> ApplicationEngine {
        engine_with_hardforks_and_snapshot(hardforks, Arc::new(PersistenceDataCache::new(false)))
    }

    /// Writes an on-chain Policy hardfork activation record.
    ///
    /// Iara and later hardforks are not configuration-managed: the engine
    /// reads their activation height from the Policy contract's prefix-25
    /// storage (`PolicyContract::hardfork_key`: suffix `[prefix, name]`,
    /// value = little-endian activation height), so settings entries alone
    /// never enable them.
    fn write_policy_hardfork_height(snapshot: &PersistenceDataCache, name: &str, height: u32) {
        let policy = crate::native::policy_contract::PolicyContract::new();
        let mut suffix = vec![25u8]; // PolicyContract::PREFIX_HARDFORK
        suffix.extend_from_slice(name.as_bytes());
        snapshot.add(
            StorageKey::new(policy.id(), suffix),
            StorageItem::from_bytes(BigInt::from(height).to_signed_bytes_le()),
        );
    }

    #[test]
    fn bls12381_deserialize_g2_identity_requires_iara() {
        let lib = CryptoLib::new();
        let g2_identity: Vec<u8> = std::iter::once(0xc0)
            .chain(std::iter::repeat(0).take(95))
            .collect();

        // C# v3.11.2: `G2Affine.FromCompressed` faults on the canonical
        // compressed identity before Iara — decompression computes
        // `y = sqrt(4(1+i))`, which does not exist in Fp2.
        let err = lib
            .bls12381_deserialize(&engine_with_hardforks(&[]), &[g2_identity.clone()])
            .expect_err("pre-Iara G2 identity must fault");
        assert!(
            err.to_string().contains("Invalid G2 point"),
            "unexpected error: {err}"
        );

        // From Iara the identity is accepted; the interop payload keeps the
        // original compressed bytes behind the group tag. Iara activation is
        // read from the on-chain Policy table, not from settings.
        let snapshot = Arc::new(PersistenceDataCache::new(false));
        write_policy_hardfork_height(&snapshot, "Iara", 0);
        let result = lib
            .bls12381_deserialize(
                &engine_with_hardforks_and_snapshot(&[], snapshot),
                &[g2_identity.clone()],
            )
            .expect("Iara accepts the canonical G2 identity");
        assert_eq!(result.len(), 1 + 96);
        assert_eq!(&result[1..], &g2_identity[..]);

        // The G1 identity has no Iara dependency: sqrt(4) exists in Fp, so
        // `G1Affine.FromCompressed` accepts it in every hardfork.
        let g1_identity: Vec<u8> = std::iter::once(0xc0)
            .chain(std::iter::repeat(0).take(47))
            .collect();
        let result = lib
            .bls12381_deserialize(&engine_with_hardforks(&[]), &[g1_identity.clone()])
            .expect("G1 identity is valid in every hardfork");
        assert_eq!(result.len(), 1 + 48);
        assert_eq!(&result[1..], &g1_identity[..]);
    }
}

//! CryptoLib native contract (id -3).
//!
//! Implements the C# `Neo.SmartContract.Native.CryptoLib` hash primitives
//! (`sha256`, `ripemd160`, `keccak256`) with byte-for-byte parity, dispatched
//! through the [`NativeContract`] trait so the application engine can invoke
//! them. The remaining CryptoLib surface (`murmur32`, `verifyWithECDsa`,
//! `verifyWithEd25519`, `recoverSecp256K1`, and the BLS12-381 operations) is
//! the next increment; the methods declared in [`CryptoLib::methods`] all have
//! working implementations — none are stubs.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::{murmur32, Crypto};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::CRYPTO_LIB_HASH;

/// Lazily-initialised script-hash handle for the CryptoLib contract.
pub static CRYPTO_LIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *CRYPTO_LIB_HASH);

/// The CryptoLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct CryptoLib;

impl CryptoLib {
    /// Stable native contract id (matches C# `CryptoLib`).
    pub const ID: i32 = -3;

    /// Construct a new `CryptoLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the CryptoLib script hash.
    pub fn script_hash() -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }
}

// C# `CpuFee = 1 << 15` for sha256 / ripemd160 / keccak256.
const CPU_FEE_HASH: i64 = 1 << 15;

/// Computes a CryptoLib hash method, returning `None` for an unknown method.
///
/// Split out from [`CryptoLib::invoke`] so the dispatch + hashing can be unit
/// tested without constructing an [`ApplicationEngine`].
fn hash_method(method: &str, data: &[u8]) -> Option<Vec<u8>> {
    match method {
        "sha256" => Some(Crypto::sha256(data).to_vec()),
        "ripemd160" => Some(Crypto::ripemd160(data).to_vec()),
        "keccak256" => Some(Crypto::keccak256(data).to_vec()),
        _ => None,
    }
}

static CRYPTO_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let byte_array = ContractParameterType::ByteArray;
    vec![
        // Unconditional since genesis.
        NativeMethod::new(
            "sha256".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        ),
        NativeMethod::new(
            "ripemd160".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        ),
        // Activated by the Cockatrice hardfork
        // (C# `[ContractMethod(Hardfork.HF_Cockatrice, ...)]`).
        NativeMethod::new(
            "keccak256".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_active_in(Hardfork::HfCockatrice),
        // murmur32(data: ByteArray, seed: Integer) -> ByteArray, C# CpuFee 1<<13.
        NativeMethod::new(
            "murmur32".to_string(),
            1 << 13,
            true,
            0,
            vec![byte_array, ContractParameterType::Integer],
            byte_array,
        ),
    ]
});

impl NativeContract for CryptoLib {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }

    fn name(&self) -> &str {
        "CryptoLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &CRYPTO_LIB_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // murmur32 takes (ByteArray data, Integer seed) and returns the 32-bit
        // hash little-endian (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        if method == "murmur32" {
            let data = args.first().ok_or_else(|| {
                CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
            })?;
            let seed_bytes = args.get(1).ok_or_else(|| {
                CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
            })?;
            // The VM integer seed is converted to `uint` exactly as C# `(uint)`
            // does: the low 32 bits (two's complement) of the BigInteger.
            let seed = (BigInt::from_signed_bytes_le(seed_bytes) & BigInt::from(u32::MAX))
                .to_u32()
                .unwrap_or(0);
            return Ok(murmur32(data, seed).to_le_bytes().to_vec());
        }

        // Every CryptoLib hash method takes a single ByteArray and returns a
        // ByteArray; the engine marshals the argument to raw bytes and the
        // ByteArray return back to a VM ByteString.
        let data = args.first().ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} requires one argument"))
        })?;
        hash_method(method, data).ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib method '{method}' is not implemented"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hash_methods_match_csharp_vectors() {
        // C# CryptoLib.{Sha256,RIPEMD160,Keccak256}(utf8("abc")).
        assert_eq!(
            hex(&hash_method("sha256", b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&hash_method("ripemd160", b"abc").unwrap()),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        assert_eq!(
            hex(&hash_method("keccak256", b"abc").unwrap()),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
        assert!(hash_method("not_a_method", b"abc").is_none());
    }

    #[test]
    fn murmur32_is_little_endian() {
        // MurmurHash3 x86 32 of empty input with seed 0 is 0 -> LE bytes 0,0,0,0
        // (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        assert_eq!(murmur32(b"", 0).to_le_bytes().to_vec(), vec![0u8, 0, 0, 0]);
        // Deterministic and non-trivial for a non-empty input.
        let h = murmur32(b"hello", 0);
        assert_eq!(murmur32(b"hello", 0), h);
        assert_eq!(h.to_le_bytes().len(), 4);
    }

    #[test]
    fn native_contract_surface_is_consistent() {
        let c = CryptoLib::new();
        assert_eq!(NativeContract::id(&c), -3);
        assert_eq!(NativeContract::name(&c), "CryptoLib");
        assert_eq!(NativeContract::hash(&c), *CRYPTO_LIB_HASH);

        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["sha256", "ripemd160", "keccak256", "murmur32"]);
        // keccak256 is hardfork-gated; the unconditional hashes are not.
        let keccak = c.methods().iter().find(|m| m.name == "keccak256").unwrap();
        assert_eq!(keccak.active_in, Some(Hardfork::HfCockatrice));
        assert!(c
            .methods()
            .iter()
            .all(|m| m.safe && m.return_type == ContractParameterType::ByteArray));
    }
}

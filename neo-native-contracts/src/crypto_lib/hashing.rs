//! CryptoLib byte hashing and murmur32 helpers.
//!
//! Keeps deterministic byte-hash behavior separate from engine-aware dispatch.

use super::CryptoLib;
use neo_crypto::{Crypto, murmur};
use neo_error::{CoreError, CoreResult};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl CryptoLib {
    /// Computes a CryptoLib hash method, returning `None` for an unknown method.
    ///
    /// Split out from [`CryptoLib::invoke_native`] so the dispatch + hashing can
    /// be unit tested without constructing an [`neo_execution::ApplicationEngine`].
    pub(super) fn hash_method(method: &str, data: &[u8]) -> Option<Vec<u8>> {
        match method {
            "sha256" => Some(Crypto::sha256(data).to_vec()),
            "ripemd160" => Some(Crypto::ripemd160(data).to_vec()),
            "keccak256" => Some(Crypto::keccak256(data).to_vec()),
            _ => None,
        }
    }

    /// C# native binding converts `uint seed` with `(uint)p.GetInteger()`, which
    /// faults on negative or wider-than-uint BigInteger values.
    fn murmur32_seed(seed_bytes: &[u8]) -> CoreResult<u32> {
        BigInt::from_signed_bytes_le(seed_bytes)
            .to_u32()
            .ok_or_else(|| CoreError::invalid_operation("CryptoLib::murmur32: seed out of range"))
    }

    pub(super) fn murmur32_method(data: &[u8], seed_bytes: &[u8]) -> CoreResult<Vec<u8>> {
        Ok(murmur::murmur32(data, Self::murmur32_seed(seed_bytes)?)
            .to_le_bytes()
            .to_vec())
    }
}

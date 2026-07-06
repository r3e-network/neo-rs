//! # neo-native-contracts::crypto_lib
//!
//! Native CryptoLib interop surface and verification helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `bls`: BLS12-381 point parsing and operation helpers.
//! - `hashing`: byte hashing and murmur32 helpers.
//! - `invoke`: native method dispatch and hardfork-gated verification routing.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `signatures`: Ed25519, ECDSA, and secp256k1 signature helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};

use crate::hashes::CRYPTO_LIB_HASH;

mod bls;
mod hashing;
mod invoke;
mod metadata;
mod signatures;

native_contract_handle!(
    /// The CryptoLib native contract.
    pub struct CryptoLib {
        id: -3,
        contract_name: "CryptoLib",
        hash: CRYPTO_LIB_HASH,
    }
);

impl NativeContract for CryptoLib {
    native_contract_identity!(CryptoLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::CRYPTO_LIB_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }

    native_contract_resolved_invoke!(metadata::CRYPTO_LIB_METHOD_BINDINGS);
}

#[cfg(test)]
#[path = "../tests/crypto_lib/mod.rs"]
mod tests;

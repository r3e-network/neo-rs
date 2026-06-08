//! CryptoLib (BLS12-381) native contract stub.

use crate::hashes::CRYPTO_LIB_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the CryptoLib contract.
pub static CRYPTO_LIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *CRYPTO_LIB_HASH);

/// Static accessor for the CryptoLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct CryptoLib;

impl CryptoLib {
    /// Stable native contract id (-11 in C# CryptoLib).
    pub const ID: i32 = -3;

    /// Construct a new `CryptoLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }
}

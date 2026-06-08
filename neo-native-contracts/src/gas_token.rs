//! GasToken (GAS) native contract stub.

use crate::hashes::GAS_TOKEN_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the GAS native contract.
pub static GAS_HASH: LazyLock<UInt160> = LazyLock::new(|| *GAS_TOKEN_HASH);

/// Static accessor for the GasToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct GasToken;

impl GasToken {
    /// Stable native contract id (-6 in C# GAS contract).
    pub const ID: i32 = -6;

    /// Construct a new `GasToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *GAS_HASH
    }
}

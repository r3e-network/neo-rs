//! Treasury native contract stub (depends on NeoToken for funding).

use crate::hashes::TREASURY_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the Treasury contract.
pub static TREASURY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *TREASURY_HASH);

/// Static accessor for the Treasury native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Treasury;

impl Treasury {
    /// Stable native contract id (-2 in C# Treasury).
    pub const ID: i32 = -11;

    /// Construct a new `Treasury` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *TREASURY_HASH_REF
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *TREASURY_HASH_REF
    }
}

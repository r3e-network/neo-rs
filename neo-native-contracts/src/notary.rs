//! Notary native contract stub.

use crate::hashes::NOTARY_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the Notary contract.
pub static NOTARY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *NOTARY_HASH);

/// Static accessor for the Notary native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Notary;

impl Notary {
    /// Stable native contract id (-12 in C# Notary).
    pub const ID: i32 = -10;

    /// Construct a new `Notary` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *NOTARY_HASH_REF
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *NOTARY_HASH_REF
    }
}

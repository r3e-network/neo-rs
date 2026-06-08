//! StdLib native contract stub.

use crate::hashes::STDLIB_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the StdLib contract.
pub static STDLIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *STDLIB_HASH);

/// Static accessor for the StdLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdLib;

impl StdLib {
    /// Stable native contract id (-3 in C# StdLib).
    pub const ID: i32 = -2;

    /// Construct a new `StdLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *STDLIB_HASH_REF
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *STDLIB_HASH_REF
    }
}

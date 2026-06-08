//! NeoToken (NEO) native contract stub.
//!
//! Provides the public API surface used by external crates
//! (consensus, plugins, etc.) without depending on the full
//! implementation in `neo-core::smart_contract::native::neo_token`.
//!
//! The stub returns empty/zero values from every storage query; a
//! real executor should wire this up to a populated native-contract
//! cache.

use crate::hashes::NEO_TOKEN_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the NEO native contract.
pub static NEO_HASH: LazyLock<UInt160> = LazyLock::new(|| *NEO_TOKEN_HASH);

/// Static accessor for the NeoToken native contract.
///
/// Mirrors the C# `NeoToken` static class. Constructing it is cheap;
/// the heavy work (cache lookup, method dispatch) is done by the
/// [`NativeContract`] trait.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (-5 in C# NEO contract).
    pub const ID: i32 = -5;

    /// Construct a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *NEO_HASH
    }
}

//! RoleManagement native contract stub.
//!
//! The stub provides the public surface used by the
//! OracleService and consensus plugins
//! (`new`, `get_designated_by_role_at`).

use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the RoleManagement contract.
pub static ROLE_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *ROLE_MANAGEMENT_HASH);

/// Static accessor for the RoleManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleManagement;

impl RoleManagement {
    /// Stable native contract id (-10 in C# RoleManagement).
    pub const ID: i32 = -8;

    /// Construct a new `RoleManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the .* native contract\.
    pub fn hash(&self) -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    /// Returns the script hash of the .* native contract (static).
    pub fn script_hash() -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    /// Look up the designated public keys for a role at a given block
    /// height. The stub returns an empty list; a real executor should
    /// wire this up to a populated native-contract cache.
    pub fn get_designated_by_role_at(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _role: Role,
        _height: u32,
    ) -> neo_error::CoreResult<Vec<neo_crypto::ECPoint>> {
        Ok(Vec::new())
    }
}

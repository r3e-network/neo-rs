//! PolicyContract native contract stub.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the PolicyContract.
pub static POLICY_HASH: LazyLock<UInt160> = LazyLock::new(|| *POLICY_CONTRACT_HASH);

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

/// Static accessor for the PolicyContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContract;

impl PolicyContract {
    /// Stable native contract id (-7 in C# Policy contract).
    pub const ID: i32 = -7;

    /// Default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
    /// Default fee per byte.
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
    /// Default max valid-until-block increment.
    pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

    /// Construct a new `PolicyContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Policy native contract.
    pub fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    /// Returns the script hash of the Policy native contract (static).
    pub fn script_hash() -> UInt160 {
        *POLICY_HASH
    }

    /// Stub: returns the max valid-until-block increment from the
    /// snapshot, or `Ok(default)` if not configured.
    pub fn get_max_valid_until_block_increment_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    /// Stub: returns the execution fee factor from the snapshot, or
    /// `Ok(DEFAULT_EXEC_FEE_FACTOR)` if not configured.
    pub fn get_exec_fee_factor_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
        _height: u32,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_EXEC_FEE_FACTOR)
    }

    /// Stub: returns the fee-per-byte from the snapshot, or
    /// `Ok(DEFAULT_FEE_PER_BYTE)` if not configured.
    pub fn get_fee_per_byte_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_FEE_PER_BYTE)
    }
}

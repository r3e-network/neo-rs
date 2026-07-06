//! PolicyContract protocol constants and event names.
//!
//! Centralizes storage prefixes, defaults, bounds, hardfork-scaled fee factors,
//! and native event names so the contract root stays focused on the native
//! contract surface.

/// C# `PolicyContract.Prefix_FeePerByte` storage prefix.
pub(in crate::policy_contract) const PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_StoragePrice` storage prefix.
pub(in crate::policy_contract) const PREFIX_STORAGE_PRICE: u8 = 19;
/// C# `PolicyContract.Prefix_ExecFeeFactor` storage prefix.
pub(in crate::policy_contract) const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
/// C# `PolicyContract.DefaultStoragePrice`.
pub(in crate::policy_contract) const DEFAULT_STORAGE_PRICE: i64 = 100_000;
/// C# `PolicyContract.Prefix_BlockedAccount` storage prefix.
pub(in crate::policy_contract) const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_WhitelistedFeeContracts` storage prefix (HF_Faun).
pub(in crate::policy_contract) const PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;
/// C# `PolicyContract.RequiredTimeForRecoverFund`: 1 year in milliseconds.
pub(in crate::policy_contract) const REQUIRED_TIME_FOR_RECOVER_FUND: u64 =
    365 * 24 * 60 * 60 * 1_000;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
pub(in crate::policy_contract) const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
pub(in crate::policy_contract) const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
pub(in crate::policy_contract) const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 =
    neo_primitives::constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT;

/// C# upper bound on fee-per-byte: 1 GAS in datoshi (`SetFeePerByte` rejects
/// anything outside `[0, 100000000]`).
pub(in crate::policy_contract) const MAX_FEE_PER_BYTE: i64 = 100_000_000;

/// C# upper bound on storage price: `PolicyContract.MaxStoragePrice`.
pub(in crate::policy_contract) const MAX_STORAGE_PRICE: i64 = 10_000_000;

/// C# `ApplicationEngine.FeeFactor` (10000): from the HF_Faun hardfork the exec
/// fee factor is stored in pico-GAS (the raw value carries this extra scaling),
/// so the legacy `getExecFeeFactor` divides it out and the bound is widened.
/// Single-sourced from `neo_execution` (C# `ApplicationEngine.FeeFactor`).
pub(crate) use neo_execution::application_engine::FEE_FACTOR;
/// C# `PolicyContract.MaxExecFeeFactor`.
pub(in crate::policy_contract) const MAX_EXEC_FEE_FACTOR: i64 = 100;

/// C# `PolicyContract.Prefix_AttributeFee` storage prefix.
pub(in crate::policy_contract) const PREFIX_ATTRIBUTE_FEE: u8 = 20;
/// C# `PolicyContract.DefaultAttributeFee`.
pub(in crate::policy_contract) const DEFAULT_ATTRIBUTE_FEE: i64 = 0;
/// C# `PolicyContract.MaxAttributeFee` (10 GAS in datoshi).
pub(in crate::policy_contract) const MAX_ATTRIBUTE_FEE: i64 = 10_0000_0000;

/// C# `PolicyContract.DefaultNotaryAssistedAttributeFee` (PolicyContract.cs:56):
/// the per-key NotaryAssisted attribute fee seeded at the HF_Echidna block.
pub(in crate::policy_contract) const DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE: i64 = 1000_0000;

/// C# `PolicyContract.MaxMillisecondsPerBlock`.
pub(in crate::policy_contract) const MAX_MILLISECONDS_PER_BLOCK: i64 = 30_000;

/// C# `PolicyContract.MaxMaxValidUntilBlockIncrement`.
pub(in crate::policy_contract) const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: i64 = 86_400;
/// C# `PolicyContract.MaxMaxTraceableBlocks`.
pub(in crate::policy_contract) const MAX_MAX_TRACEABLE_BLOCKS: i64 = 2_102_400;

pub(crate) const POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT: &str = "MillisecondsPerBlockChanged";
pub(crate) const POLICY_WHITELIST_FEE_CHANGED_EVENT: &str = "WhitelistFeeChanged";
pub(crate) const POLICY_RECOVERED_FUND_EVENT: &str = "RecoveredFund";

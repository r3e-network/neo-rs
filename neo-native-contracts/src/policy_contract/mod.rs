//! # neo-native-contracts::policy_contract
//!
//! Native Policy contract fee, account, and storage policy behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `dispatch`: Native method dispatch and runtime side effects.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

mod dispatch;
mod metadata;
mod storage;

/// C# `PolicyContract.Prefix_FeePerByte` storage prefix.
const PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_StoragePrice` storage prefix.
const PREFIX_STORAGE_PRICE: u8 = 19;
/// C# `PolicyContract.Prefix_ExecFeeFactor` storage prefix.
const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
/// C# `PolicyContract.DefaultStoragePrice`.
const DEFAULT_STORAGE_PRICE: i64 = 100_000;
/// C# `PolicyContract.Prefix_BlockedAccount` storage prefix.
const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_WhitelistedFeeContracts` storage prefix (HF_Faun).
const PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;
/// C# `PolicyContract.RequiredTimeForRecoverFund`: 1 year in milliseconds.
const REQUIRED_TIME_FOR_RECOVER_FUND: u64 = 365 * 24 * 60 * 60 * 1_000;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 =
    neo_primitives::constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT;

native_contract_handle!(
    /// Static accessor for the PolicyContract native contract.
    pub struct PolicyContract {
        id: -7,
        contract_name: "PolicyContract",
        hash: POLICY_CONTRACT_HASH,
    }
);

/// C# upper bound on fee-per-byte: 1 GAS in datoshi (`SetFeePerByte` rejects
/// anything outside `[0, 100000000]`).
const MAX_FEE_PER_BYTE: i64 = 100_000_000;

/// C# upper bound on storage price: `PolicyContract.MaxStoragePrice`.
const MAX_STORAGE_PRICE: i64 = 10_000_000;

/// C# `ApplicationEngine.FeeFactor` (10000): from the HF_Faun hardfork the exec
/// fee factor is stored in pico-GAS (the raw value carries this extra scaling),
/// so the legacy `getExecFeeFactor` divides it out and the bound is widened.
/// Single-sourced from `neo_execution` (C# `ApplicationEngine.FeeFactor`).
pub(crate) use neo_execution::application_engine::FEE_FACTOR;
/// C# `PolicyContract.MaxExecFeeFactor`.
const MAX_EXEC_FEE_FACTOR: i64 = 100;

/// C# `PolicyContract.Prefix_AttributeFee` storage prefix.
const PREFIX_ATTRIBUTE_FEE: u8 = 20;
/// C# `PolicyContract.DefaultAttributeFee`.
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;
/// C# `PolicyContract.MaxAttributeFee` (10 GAS in datoshi).
const MAX_ATTRIBUTE_FEE: i64 = 10_0000_0000;

/// C# `PolicyContract.DefaultNotaryAssistedAttributeFee` (PolicyContract.cs:56):
/// the per-key NotaryAssisted attribute fee seeded at the HF_Echidna block.
const DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE: i64 = 1000_0000;

/// C# `PolicyContract.MaxMillisecondsPerBlock`.
const MAX_MILLISECONDS_PER_BLOCK: i64 = 30_000;

/// C# `PolicyContract.MaxMaxValidUntilBlockIncrement`.
const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: i64 = 86_400;
/// C# `PolicyContract.MaxMaxTraceableBlocks`.
const MAX_MAX_TRACEABLE_BLOCKS: i64 = 2_102_400;
pub(crate) const POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT: &str = "MillisecondsPerBlockChanged";
pub(crate) const POLICY_WHITELIST_FEE_CHANGED_EVENT: &str = "WhitelistFeeChanged";
pub(crate) const POLICY_RECOVERED_FUND_EVENT: &str = "RecoveredFund";

impl NativeContract for PolicyContract {
    native_contract_identity!(PolicyContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::POLICY_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::POLICY_CONTRACT_EVENTS
    }

    /// The `ApplicationEngine` contract-invocation gate (C#
    /// `ApplicationEngine.CallContract` -> `NativeContract.Policy.IsBlocked`):
    /// `snapshot.Contains(key(Prefix_BlockedAccount, hash))`. Native contracts can
    /// never be in the blocked list (`blockAccount` rejects them), so no special
    /// casing is needed. Without this override the trait default `Ok(false)` would
    /// let a blocked contract be invoked, diverging from C#.
    fn is_contract_blocked(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        Ok(snapshot
            .get(&Self::blocked_account_key(contract_hash))
            .is_some())
    }

    /// C# `PolicyContract.InitializeAsync(engine, hardfork)` for `hardfork ==
    /// ActiveIn` (PolicyContract.cs:137-143; Policy is genesis-active, so this
    /// runs while persisting block 0): seed `Prefix_FeePerByte` (1000),
    /// `Prefix_ExecFeeFactor` (30), and `Prefix_StoragePrice` (100000). The
    /// HF_Echidna / HF_Faun re-initialization branches live in
    /// `initialize_for_hardfork`, triggered by `ContractManagement`'s
    /// `on_persist` at those hardfork blocks.
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::fee_per_byte_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_FEE_PER_BYTE,
            ))),
        );
        snapshot.add(
            Self::exec_fee_factor_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_EXEC_FEE_FACTOR,
            ))),
        );
        snapshot.add(
            Self::storage_price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_STORAGE_PRICE,
            ))),
        );
        Ok(())
    }

    /// C# `PolicyContract.IsWhitelistFeeContract(snapshot, contractHash,
    /// method, out fixedFee)`, reached by the engine's contract-call fee logic
    /// through the native-contract seam: the contract must exist in
    /// ContractManagement, the `(method, paramCount)` descriptor must resolve,
    /// and a `Prefix_WhitelistedFeeContracts ++ hash ++ offset` entry must be
    /// stored — then its `FixedFee` applies instead of per-instruction fees.
    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        let Some(contract) =
            crate::ContractManagement::get_contract_from_snapshot(snapshot, contract_hash)?
        else {
            return Ok(None);
        };
        let Some(descriptor) = contract
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == method && m.parameters.len() == param_count as usize)
        else {
            return Ok(None);
        };
        match snapshot.get(&Self::whitelist_fee_key(contract_hash, descriptor.offset)) {
            Some(item) => Ok(Some(
                Self::decode_whitelisted_contract(&item.value_bytes())?.fixed_fee,
            )),
            None => Ok(None),
        }
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_policy_method(engine, method, args)
    }
}

#[cfg(test)]
#[path = "../tests/policy_contract/mod.rs"]
mod tests;

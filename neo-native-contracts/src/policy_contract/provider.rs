//! Policy provider seams consumed by the execution engine.
//!
//! Keeps engine-facing read paths out of the contract root while preserving
//! blocked-contract gating and whitelisted-fee resolution against stored
//! contract manifests.

use super::PolicyContract;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;

impl PolicyContract {
    /// The `ApplicationEngine` contract-invocation gate (C#
    /// `ApplicationEngine.CallContract` -> `NativeContract.Policy.IsBlocked`):
    /// `snapshot.Contains(key(Prefix_BlockedAccount, hash))`. Native contracts can
    /// never be in the blocked list (`blockAccount` rejects them), so no special
    /// casing is needed. Without this override the trait default `Ok(false)` would
    /// let a blocked contract be invoked, diverging from C#.
    pub(super) fn is_contract_blocked_native(
        &self,
        snapshot: &DataCache,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        Ok(snapshot
            .get(&Self::blocked_account_key(contract_hash))
            .is_some())
    }

    /// C# `PolicyContract.IsWhitelistFeeContract(snapshot, contractHash,
    /// method, out fixedFee)`, reached by the engine's contract-call fee logic
    /// through the native-contract seam: the contract must exist in
    /// ContractManagement, the `(method, paramCount)` descriptor must resolve,
    /// and a `Prefix_WhitelistedFeeContracts ++ hash ++ offset` entry must be
    /// stored - then its `FixedFee` applies instead of per-instruction fees.
    pub(super) fn whitelisted_fee_native(
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
}

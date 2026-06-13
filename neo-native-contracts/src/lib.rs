//! # Neo Native Contracts
//!
//! Canonical home for the 11 standard Neo native contracts (NEO, GAS,
//! Policy, Oracle, Ledger, ContractManagement, CryptoLib, Notary,
//! RoleManagement, StdLib, Treasury) and the shared
//! `NativeContract` infrastructure.
//!
//! Each native-contract submodule provides a Rust handle type
//! (`NeoToken`, `GasToken`, …) that exposes:
//!
//! - the well-known script hash ([`hashes`])
//! - a stable integer id (`Self::ID`)
//! - the storage-query surface needed by external plugins and
//!   services (`get_request`, `get_designated_by_role_at`, …)
//!
//! The implementations mirror the C# `Neo.SmartContract.Native.*`
//! storage layout (prefix bytes, account-hash encoding, value
//! serialization) so the Rust native-contract surface is
//! byte-compatible with the canonical C# node.

#![allow(dead_code)]
// Several module-level imports are consumed only by the `#[cfg(test)]` modules
// (via `use super::*`); they read as unused in the non-test build, so this
// keeps the crate warning-clean without scattering `#[cfg(test)]` on imports.
#![allow(unused_imports)]

pub use neo_execution::{
    HardforkActivable, NativeContract, NativeContractsCache, NativeContractsCacheEntry,
    NativeEvent, NativeMethod, NativeRegistry, is_active_for,
};

mod catalog;
pub mod contract_management;
pub mod crypto_lib;
mod dotnet_graphemes;
mod dotnet_text_segmentation;
pub mod gas_token;
pub mod hashes;
pub mod ledger_contract;
pub mod native_contract;
pub mod neo_token;
pub mod notary;
pub mod oracle_contract;
pub mod policy_contract;
pub mod provider;

pub(crate) mod args;
pub(crate) mod committee;
pub(crate) mod keys;

#[cfg(test)]
pub(crate) mod test_support;
pub mod role;
pub mod role_management;
pub mod std_lib;
pub mod treasury;

pub use catalog::{StandardNativeContractSpec, standard_native_contract_specs};
pub use contract_management::ContractManagement;
pub use crypto_lib::CryptoLib;
pub use gas_token::GasToken;
pub use ledger_contract::LedgerContract;
pub use neo_token::NeoToken;
pub use notary::Notary;
pub use oracle_contract::{OracleContract, OracleRequest};
pub use policy_contract::PolicyContract;
pub use provider::{StandardNativeProvider, install};
pub use role::Role;
pub use role_management::RoleManagement;
pub use std_lib::StdLib;
pub use treasury::Treasury;

/// Reads a native-contract integer setting from `snapshot` under
/// `(contract_id, prefix)`, returning `default` when the key is absent.
///
/// Native settings (fee-per-byte, storage price, oracle price, …) are stored as
/// C# `BigInteger` values in signed little-endian bytes; C# reads them via
/// `(long)(BigInteger)snapshot[key]`. The value is written at contract
/// initialization, so absence only happens pre-genesis / in tests, where the
/// caller supplies the same default the init routine would write.
pub(crate) fn read_storage_int(
    snapshot: &neo_storage::persistence::DataCache,
    contract_id: i32,
    prefix: u8,
    default: i64,
) -> neo_error::CoreResult<i64> {
    use num_traits::ToPrimitive;
    let key = neo_storage::StorageKey::new(contract_id, vec![prefix]);
    match snapshot.get(&key) {
        Some(item) => num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                neo_error::CoreError::invalid_operation("native storage integer out of range")
            }),
        None => Ok(default),
    }
}

/// Encodes a `BigInteger` for native-contract storage exactly like C#
/// `StorageItem`/`BigInteger.ToByteArrayStandard()`: **empty bytes for zero**,
/// otherwise the signed little-endian two's-complement form. `num-bigint`'s
/// `to_signed_bytes_le()` matches the non-zero form but yields `[0x00]` for
/// zero, which would diverge the raw stored bytes (and so the state root)
/// anywhere a stored counter or setting can legitimately reach zero (e.g.
/// `_votersCount` after the last un-vote, `gasPerBlock = 0`, `feePerByte = 0`).
/// Reads are unaffected: `BigInt::from_signed_bytes_le(&[])` is zero.
pub(crate) fn bigint_to_storage_bytes(value: &num_bigint::BigInt) -> Vec<u8> {
    use num_traits::Zero;
    if value.is_zero() {
        Vec::new()
    } else {
        value.to_signed_bytes_le()
    }
}

/// The `Transfer` event declared on the C# `FungibleToken` base constructor
/// (FungibleToken.cs:59-62) and inherited — via the base-type constructor
/// concat in `NativeContract`'s reflection — by both NEO and GAS at order 0:
/// `Transfer(from: Hash160, to: Hash160, amount: Integer)`, ungated.
pub(crate) fn fungible_token_transfer_event() -> NativeEvent {
    use neo_primitives::ContractParameterType;
    NativeEvent::new(
        0,
        "Transfer",
        &[
            ("from", ContractParameterType::Hash160),
            ("to", ContractParameterType::Hash160),
            ("amount", ContractParameterType::Integer),
        ],
    )
}

/// C# `FungibleToken.Prefix_TotalSupply`.
pub(crate) const NEP17_PREFIX_TOTAL_SUPPLY: u8 = 11;
/// C# `FungibleToken.Prefix_Account`.
pub(crate) const NEP17_PREFIX_ACCOUNT: u8 = 20;

/// Reads a NEP-17 account balance — the `Balance` field (index 0) of the
/// account-state struct stored under `(contract_id, [20] ++ account)` — returning
/// 0 when the account has no entry. Matches C# `FungibleToken.BalanceOf`, which
/// reads `item.GetInteroperable<TState>().Balance` and returns
/// `BigInteger.Zero` when the key is absent.
pub(crate) fn read_nep17_balance(
    snapshot: &neo_storage::persistence::DataCache,
    contract_id: i32,
    account: &neo_primitives::UInt160,
) -> neo_error::CoreResult<num_bigint::BigInt> {
    let mut key_bytes = vec![NEP17_PREFIX_ACCOUNT];
    key_bytes.extend_from_slice(&account.to_bytes());
    let key = neo_storage::StorageKey::new(contract_id, key_bytes);

    let Some(item) = snapshot.get(&key) else {
        return Ok(num_bigint::BigInt::from(0));
    };
    let state = neo_serialization::BinarySerializer::deserialize(
        &item.value_bytes(),
        &neo_vm_rs::ExecutionEngineLimits::default(),
        None,
    )
    .map_err(|e| neo_error::CoreError::deserialization(format!("NEP-17 account state: {e}")))?;
    let neo_vm::StackItem::Struct(fields) = state else {
        return Err(neo_error::CoreError::invalid_data(
            "NEP-17 account state is not a struct",
        ));
    };
    let items = fields.items();
    let balance = items
        .first()
        .ok_or_else(|| neo_error::CoreError::invalid_data("NEP-17 account state is empty"))?;
    balance
        .as_int()
        .map_err(|e| neo_error::CoreError::invalid_data(format!("NEP-17 balance: {e}")))
}

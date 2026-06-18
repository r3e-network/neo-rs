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

pub mod role;
pub mod role_management;
pub mod std_lib;
#[cfg(test)]
pub(crate) mod test_support;
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

use neo_vm::Interoperable;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;

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
    let key = neo_storage::StorageKey::create(contract_id, prefix);
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

/// C# `AccountState`: the base native-token account state
/// `Struct[Balance]`. `NeoAccountState` extends this shape with governance
/// fields, but the balance projection is common to NEO and GAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccountState {
    pub(crate) balance: BigInt,
}

impl AccountState {
    pub(crate) fn new(balance: BigInt) -> Self {
        Self { balance }
    }

    pub(crate) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            0,
            vec![StackValue::BigInteger(self.balance.to_signed_bytes_le())],
        )
    }

    pub(crate) fn from_stack_value(stack_value: StackValue) -> neo_error::CoreResult<Self> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(neo_error::CoreError::invalid_data(
                "NEP-17 account state is not a struct",
            ));
        };
        let balance = items
            .first()
            .ok_or_else(|| neo_error::CoreError::invalid_data("NEP-17 account state is empty"))?;
        let balance = neo_vm_rs::stack_value_as_bigint(balance)
            .map_err(|e| neo_error::CoreError::invalid_data(format!("NEP-17 balance: {e}")))?;
        Ok(Self { balance })
    }
}

neo_vm::impl_interoperable_via_stack_value!(AccountState);

/// Deserializes a stored NEP-17 account-state struct (`Struct[Balance]`) from
/// its on-chain byte representation. Shared by [`read_nep17_balance`] and the
/// per-token account readers (`GasToken::read_gas_account`,
/// `NeoToken::read_account_state`) to avoid duplicating the
/// `deserialize_stack_value_with_limits` + `AccountState::from_stack_value`
/// plumbing in every caller.
pub(crate) fn deserialize_account_state(
    bytes: &[u8],
) -> neo_error::CoreResult<AccountState> {
    let limits = neo_vm_rs::ExecutionEngineLimits::default();
    let decoded = neo_serialization::BinarySerializer::deserialize_stack_value_with_limits(
        bytes,
        limits.max_item_size as usize,
        limits.max_stack_size as usize,
    )
    .map_err(|e| neo_error::CoreError::deserialization(format!("NEP-17 account state: {e}")))?;
    AccountState::from_stack_value(decoded)
}

/// Serializes a NEP-17 account-state struct to its on-chain byte form.
/// Companion of [`deserialize_account_state`].
pub(crate) fn serialize_account_state(
    state: &AccountState,
) -> neo_error::CoreResult<Vec<u8>> {
    neo_serialization::BinarySerializer::serialize_stack_value_default(&state.to_stack_value())
        .map_err(|e| neo_error::CoreError::serialization(format!("NEP-17 account state: {e}")))
}

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
    let key = neo_storage::StorageKey::create_with_uint160(contract_id, NEP17_PREFIX_ACCOUNT, account);

    let Some(item) = snapshot.get(&key) else {
        return Ok(num_bigint::BigInt::from(0));
    };
    let state = deserialize_account_state(item.value_bytes().as_ref())?;
    Ok(state.balance)
}

#[cfg(test)]
mod tests {
    use super::AccountState;
    use neo_vm::Interoperable;
    use neo_vm_rs::StackValue;
    use num_bigint::BigInt;

    #[test]
    fn account_state_interoperable_projection_matches_csharp_shape() {
        let state = AccountState::new(BigInt::from(12345));
        let expected_value = StackValue::Struct(
            0,
            vec![StackValue::BigInteger(
                BigInt::from(12345).to_signed_bytes_le(),
            )],
        );

        assert_eq!(state.to_stack_value(), expected_value);

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        assert_eq!(trait_value, expected_value);

        let mut parsed = AccountState::new(BigInt::from(0));
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed, state);

        assert!(AccountState::from_stack_value(StackValue::Array(0, vec![])).is_err());
        assert!(AccountState::from_stack_value(StackValue::Struct(0, vec![])).is_err());
    }

    #[test]
    fn nep17_balance_reader_uses_stack_value_projection() {
        let source = include_str!("lib.rs");
        let start = source
            .find("pub(crate) fn read_nep17_balance(")
            .expect("read_nep17_balance helper exists");
        let end = source[start..]
            .find("#[cfg(test)]")
            .map(|offset| start + offset)
            .expect("tests follow read_nep17_balance");
        let helper = &source[start..end];

        // After the FungibleToken-helper extraction, read_nep17_balance delegates
        // (de)serialization to the shared deserialize_account_state helper rather
        // than inlining the BinarySerializer plumbing. The contract here is that
        // the reader stays a thin wrapper: key build + get + shared helper.
        assert!(helper.contains("deserialize_account_state"));
        assert!(helper.contains("StorageKey::create_with_uint160"));
        assert!(!helper.contains("StackValue::Struct"));
        assert!(!helper.contains("stack_value_as_bigint"));
        assert!(!helper.contains("BinarySerializer::deserialize("));
        assert!(!helper.contains("neo_vm::StackItem::Struct"));
    }
}

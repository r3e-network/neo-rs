//! Shared NEP helpers for native contracts.
//!
//! This module keeps NEP standard names, NEP-17 ABI descriptors, transfer
//! notification payloads, and fungible-token account codecs out of the crate
//! root while preserving a single implementation for NEO, GAS, Notary, and
//! Treasury.

use neo_execution::{NativeEvent, NativeMethod};
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;

/// The `Transfer` event declared on the C# `FungibleToken` base constructor
/// (FungibleToken.cs:59-62) and inherited via the base-type constructor concat
/// in `NativeContract` reflection by both NEO and GAS at order 0:
/// `Transfer(from: Hash160, to: Hash160, amount: Integer)`, ungated.
pub(crate) const NEP17_TRANSFER_EVENT: &str = "Transfer";
pub(crate) const NEP17_PAYMENT_METHOD: &str = "onNEP17Payment";
pub(crate) const NEP11_PAYMENT_METHOD: &str = "onNEP11Payment";

pub(crate) fn fungible_token_transfer_event() -> NativeEvent {
    use neo_primitives::ContractParameterType;
    NativeEvent::new(
        0,
        NEP17_TRANSFER_EVENT,
        &[
            ("from", ContractParameterType::Hash160),
            ("to", ContractParameterType::Hash160),
            ("amount", ContractParameterType::Integer),
        ],
    )
}

fn nep17_transfer_account_item(account: Option<&neo_primitives::UInt160>) -> StackItem {
    match account {
        Some(account) => StackItem::from_byte_string(account.to_bytes()),
        None => StackItem::null(),
    }
}

pub(crate) fn nep17_transfer_notification_state(
    from: Option<&neo_primitives::UInt160>,
    to: Option<&neo_primitives::UInt160>,
    amount: &BigInt,
) -> Vec<StackItem> {
    vec![
        nep17_transfer_account_item(from),
        nep17_transfer_account_item(to),
        StackItem::from_int(amount.clone()),
    ]
}

pub(crate) fn nep17_payment_callback_args(
    from: Option<&neo_primitives::UInt160>,
    amount: &BigInt,
    data: StackItem,
) -> Vec<StackItem> {
    vec![
        nep17_transfer_account_item(from),
        StackItem::from_int(amount.clone()),
        data,
    ]
}

pub(crate) fn nep17_payment_data_item(
    data: &[u8],
    context: &str,
) -> neo_error::CoreResult<StackItem> {
    if data.is_empty() {
        return Ok(StackItem::null());
    }
    neo_serialization::BinarySerializer::deserialize_default(data)
        .map_err(|e| neo_error::CoreError::deserialization(format!("{context}: {e}")))
}

pub(crate) fn nep17_symbol_method() -> NativeMethod {
    NativeMethod::new(
        "symbol",
        0,
        true,
        0,
        Vec::new(),
        neo_primitives::ContractParameterType::String,
    )
}

pub(crate) fn nep17_decimals_method() -> NativeMethod {
    NativeMethod::new(
        "decimals",
        0,
        true,
        0,
        Vec::new(),
        neo_primitives::ContractParameterType::Integer,
    )
}

pub(crate) fn nep17_total_supply_method(read_states: u8) -> NativeMethod {
    NativeMethod::new(
        "totalSupply",
        1 << 15,
        true,
        read_states,
        Vec::new(),
        neo_primitives::ContractParameterType::Integer,
    )
}

pub(crate) fn nep17_balance_of_method(read_states: u8) -> NativeMethod {
    NativeMethod::new(
        "balanceOf",
        1 << 15,
        true,
        read_states,
        vec![neo_primitives::ContractParameterType::Hash160],
        neo_primitives::ContractParameterType::Integer,
    )
    .with_parameter_names(["account"])
}

pub(crate) fn nep17_transfer_method() -> NativeMethod {
    use neo_primitives::ContractParameterType::{Any, Boolean, Hash160, Integer};
    NativeMethod::new(
        "transfer",
        1 << 17,
        false,
        (neo_primitives::CallFlags::STATES
            | neo_primitives::CallFlags::ALLOW_CALL
            | neo_primitives::CallFlags::ALLOW_NOTIFY)
            .bits(),
        vec![Hash160, Hash160, Integer, Any],
        Boolean,
    )
    .with_storage_fee(50)
    .with_parameter_names(["from", "to", "amount", "data"])
}

pub(crate) fn nep17_payment_method(
    cpu_fee: i64,
    safe: bool,
    required_call_flags: u8,
) -> NativeMethod {
    use neo_primitives::ContractParameterType::{Any, Hash160, Integer, Void};
    NativeMethod::new(
        NEP17_PAYMENT_METHOD.to_owned(),
        cpu_fee,
        safe,
        required_call_flags,
        vec![Hash160, Integer, Any],
        Void,
    )
    .with_parameter_names(["from", "amount", "data"])
}

pub(crate) fn nep11_payment_method(
    cpu_fee: i64,
    safe: bool,
    required_call_flags: u8,
) -> NativeMethod {
    use neo_primitives::ContractParameterType::{Any, ByteArray, Hash160, Integer, Void};
    NativeMethod::new(
        NEP11_PAYMENT_METHOD.to_owned(),
        cpu_fee,
        safe,
        required_call_flags,
        vec![Hash160, Integer, ByteArray, Any],
        Void,
    )
    .with_parameter_names(["from", "amount", "tokenId", "data"])
}

pub(crate) const NEP17_STANDARD: &str = "NEP-17";
pub(crate) const NEP26_STANDARD: &str = "NEP-26";
pub(crate) const NEP27_STANDARD: &str = "NEP-27";
pub(crate) const NEP30_STANDARD: &str = "NEP-30";

pub(crate) fn native_supported_standards(standards: &[&str]) -> Vec<String> {
    standards
        .iter()
        .map(|standard| (*standard).to_owned())
        .collect()
}

/// C# `FungibleToken.Prefix_TotalSupply`.
pub(crate) const NEP17_PREFIX_TOTAL_SUPPLY: u8 = 11;
/// C# `FungibleToken.Prefix_Account`.
pub(crate) const NEP17_PREFIX_ACCOUNT: u8 = 20;

/// The shared NEP-17 total-supply storage key
/// `(contract_id, [Prefix_TotalSupply])`.
pub(crate) fn nep17_total_supply_key(contract_id: i32) -> neo_storage::StorageKey {
    crate::keys::prefixed_key(contract_id, NEP17_PREFIX_TOTAL_SUPPLY, &[])
}

/// The shared NEP-17 account storage key
/// `(contract_id, [Prefix_Account] ++ account)`.
pub(crate) fn nep17_account_key(
    contract_id: i32,
    account: &neo_primitives::UInt160,
) -> neo_storage::StorageKey {
    crate::keys::prefixed_hash160_key(contract_id, NEP17_PREFIX_ACCOUNT, account)
}

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
        StackValue::Struct(vec![StackValue::BigInteger(
            self.balance.to_signed_bytes_le(),
        )])
    }

    pub(crate) fn from_stack_value(stack_value: StackValue) -> neo_error::CoreResult<Self> {
        let decoder =
            crate::support::codec::StructDecoder::new(&stack_value, "NEP-17 account state")?;
        let balance = decoder.bigint(0, "balance")?;
        Ok(Self { balance })
    }
}

neo_vm::impl_interoperable_via_stack_value!(AccountState);

/// Deserializes a stored NEP-17 account-state struct (`Struct[Balance]`) from
/// its on-chain byte representation. Shared by [`read_nep17_balance`] and the
/// per-token account readers (`GasToken::read_gas_account`,
/// `NeoToken::read_account_state`) to avoid duplicating the
/// `decode_stack_value` + `AccountState::from_stack_value` plumbing in every
/// caller.
pub(crate) fn deserialize_account_state(bytes: &[u8]) -> neo_error::CoreResult<AccountState> {
    let decoded = crate::support::codec::decode_stack_value(bytes, "NEP-17 account state")?;
    AccountState::from_stack_value(decoded)
}

/// Serializes a NEP-17 account-state struct to its on-chain byte form.
/// Companion of [`deserialize_account_state`].
pub(crate) fn serialize_account_state(state: &AccountState) -> neo_error::CoreResult<Vec<u8>> {
    crate::support::codec::encode_storage_struct(state, "NEP-17 account state")
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
    let key = nep17_account_key(contract_id, account);

    let Some(item) = snapshot.get(&key) else {
        return Ok(num_bigint::BigInt::from(0));
    };
    let state = deserialize_account_state(item.value_bytes().as_ref())?;
    Ok(state.balance)
}

/// Reads the NEP-17 total supply stored under `(contract_id, [11])`, returning
/// 0 when the supply key is absent. Matches C# `FungibleToken.TotalSupply`,
/// which reads the raw `StorageItem` as a `BigInteger`.
pub(crate) fn read_nep17_total_supply(
    snapshot: &neo_storage::persistence::DataCache,
    contract_id: i32,
) -> num_bigint::BigInt {
    let key = nep17_total_supply_key(contract_id);
    snapshot
        .get(&key)
        .map(|item| num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_else(|| num_bigint::BigInt::from(0))
}

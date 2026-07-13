use super::{
    AccountState, NEP11_PAYMENT_METHOD, NEP17_PAYMENT_METHOD, NEP17_STANDARD, NEP26_STANDARD,
    NEP27_STANDARD, NEP30_STANDARD, native_supported_standards, nep17_account_key,
    nep17_payment_callback_args, nep17_payment_data_item, nep17_total_supply_key,
    nep17_transfer_notification_state, read_nep17_total_supply,
};
use neo_primitives::{ContractParameterType, UInt160};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::Interoperable;
use neo_vm::StackValue;
use num_bigint::BigInt;

#[path = "style/mod.rs"]
mod style;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm::StackValue, b: &neo_vm::StackValue) -> bool {
    a.structural_eq(b)
}

#[test]
fn account_state_interoperable_projection_matches_csharp_shape() {
    let state = AccountState::new(BigInt::from(12345));
    let expected_value = StackValue::Struct(
        neo_vm::next_stack_item_id(),
        vec![StackValue::BigInteger(
            BigInt::from(12345).to_signed_bytes_le(),
        )],
    );

    let projected = state.to_stack_value();
    assert!(
        stack_value_struct_eq(&projected, &expected_value),
        "structural StackValue mismatch: {projected:?} vs {expected_value:?}"
    );

    let trait_value = Interoperable::to_stack_value(&state).unwrap();
    assert!(
        stack_value_struct_eq(&trait_value, &expected_value),
        "structural StackValue mismatch: {trait_value:?} vs {expected_value:?}"
    );

    let mut parsed = AccountState::new(BigInt::from(0));
    Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
    assert_eq!(parsed, state);

    assert!(
        AccountState::from_stack_value(StackValue::Array(neo_vm::next_stack_item_id(), vec![],))
            .is_err()
    );
    assert!(
        AccountState::from_stack_value(StackValue::Struct(neo_vm::next_stack_item_id(), vec![],))
            .is_err()
    );
}

#[test]
fn nep17_key_helpers_match_csharp_prefix_layout() {
    let account = UInt160::from_bytes(&[0xAB; 20]).unwrap();

    let account_key = nep17_account_key(-6, &account);
    assert_eq!(account_key.id(), -6);
    assert_eq!(account_key.key()[0], super::NEP17_PREFIX_ACCOUNT);
    assert_eq!(&account_key.key()[1..], account.as_bytes());
    assert_eq!(
        account_key,
        crate::keys::prefixed_hash160_key(-6, super::NEP17_PREFIX_ACCOUNT, &account)
    );

    let supply_key = nep17_total_supply_key(-6);
    assert_eq!(supply_key.id(), -6);
    assert_eq!(supply_key.key(), &[super::NEP17_PREFIX_TOTAL_SUPPLY]);
    assert_eq!(
        supply_key,
        crate::keys::prefixed_key(-6, super::NEP17_PREFIX_TOTAL_SUPPLY, &[])
    );
}

#[test]
fn nep17_key_helpers_use_shared_storage_key_builders() {
    let source = include_str!("../support/token/nep.rs");
    let start = source
        .find("pub(crate) fn nep17_total_supply_key(")
        .expect("NEP-17 key helpers exist");
    let end = source[start..]
        .find("/// C# `AccountState`")
        .map(|offset| start + offset)
        .expect("account state follows NEP-17 key helpers");
    let helpers = &source[start..end];

    assert!(helpers.contains("crate::keys::prefixed_key"));
    assert!(helpers.contains("crate::keys::prefixed_hash160_key"));
    assert!(!helpers.contains("StorageKey::create("));
    assert!(!helpers.contains("StorageKey::create_with_uint160("));
}

#[test]
fn nep17_balance_reader_uses_stack_value_projection() {
    let source = include_str!("../support/token/nep.rs");
    let start = source
        .find("pub(crate) fn read_nep17_balance(")
        .expect("read_nep17_balance helper exists");
    let end = source[start..]
        .find("/// Reads the NEP-17 total supply")
        .map(|offset| start + offset)
        .expect("total supply reader follows read_nep17_balance");
    let helper = &source[start..end];

    // After the FungibleToken-helper extraction, read_nep17_balance delegates
    // (de)serialization to the shared deserialize_account_state helper rather
    // than inlining the BinarySerializer plumbing. The contract here is that
    // the reader stays a thin wrapper: key build + get + shared helper.
    assert!(helper.contains("deserialize_account_state"));
    assert!(helper.contains("nep17_account_key"));
    assert!(!helper.contains("StackValue::Struct"));
    assert!(!helper.contains("stack_value_as_bigint"));
    assert!(!helper.contains("BinarySerializer::deserialize("));
    assert!(!helper.contains("neo_vm::StackItem::Struct"));
}

#[test]
fn nep17_total_supply_reader_matches_fungible_token_default() {
    let cache = DataCache::new(false);
    assert_eq!(read_nep17_total_supply(&cache, -6), BigInt::from(0));

    cache.add(
        nep17_total_supply_key(-6),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(123456789i64))),
    );
    assert_eq!(
        read_nep17_total_supply(&cache, -6),
        BigInt::from(123456789i64)
    );
}

#[test]
fn native_supported_standards_helper_preserves_manifest_order() {
    assert_eq!(
        native_supported_standards(&[NEP17_STANDARD, NEP27_STANDARD]),
        ["NEP-17", "NEP-27"]
    );
    assert_eq!(
        native_supported_standards(&[NEP26_STANDARD, NEP27_STANDARD, NEP30_STANDARD]),
        ["NEP-26", "NEP-27", "NEP-30"]
    );
}

#[test]
fn nep17_transfer_notification_state_matches_fungible_token_shape() {
    let from = UInt160::from([0x11; 20]);
    let to = UInt160::from([0x22; 20]);
    let amount = BigInt::from(42);

    let transfer = nep17_transfer_notification_state(Some(&from), Some(&to), &amount);
    assert_eq!(transfer.len(), 3);
    assert_eq!(transfer[0].as_bytes().unwrap(), from.to_bytes());
    assert_eq!(transfer[1].as_bytes().unwrap(), to.to_bytes());
    assert_eq!(transfer[2].as_int().unwrap(), amount);

    let mint = nep17_transfer_notification_state(None, Some(&to), &amount);
    assert!(matches!(mint[0], neo_vm::StackItem::Null));
    assert_eq!(mint[1].as_bytes().unwrap(), to.to_bytes());
    assert_eq!(mint[2].as_int().unwrap(), amount);

    let burn = nep17_transfer_notification_state(Some(&from), None, &amount);
    assert_eq!(burn[0].as_bytes().unwrap(), from.to_bytes());
    assert!(matches!(burn[1], neo_vm::StackItem::Null));
    assert_eq!(burn[2].as_int().unwrap(), amount);
}

#[test]
fn nep17_payment_callback_args_match_on_payment_shape() {
    assert_eq!(NEP17_PAYMENT_METHOD, "onNEP17Payment");

    let from = UInt160::from([0x33; 20]);
    let amount = BigInt::from(99);
    let data = neo_vm::StackItem::from_byte_string(vec![0xaa, 0xbb]);

    let transfer = nep17_payment_callback_args(Some(&from), &amount, data.clone());
    assert_eq!(transfer.len(), 3);
    assert_eq!(transfer[0].as_bytes().unwrap(), from.to_bytes());
    assert_eq!(transfer[1].as_int().unwrap(), amount);
    assert_eq!(transfer[2].as_bytes().unwrap(), vec![0xaa, 0xbb]);

    let mint = nep17_payment_callback_args(None, &amount, neo_vm::StackItem::null());
    assert!(matches!(mint[0], neo_vm::StackItem::Null));
    assert_eq!(mint[1].as_int().unwrap(), amount);
    assert!(matches!(mint[2], neo_vm::StackItem::Null));
}

#[test]
fn nep17_payment_data_item_round_trips_any_payload() {
    let empty = nep17_payment_data_item(&[], "test data").expect("empty data");
    assert!(matches!(empty, neo_vm::StackItem::Null));

    let original = neo_vm::StackItem::from_byte_string(vec![0xaa, 0xbb]);
    let bytes = neo_serialization::BinarySerializer::serialize(
        &original,
        &neo_vm::ExecutionEngineLimits::default(),
    )
    .expect("serialize stack item");
    let restored = nep17_payment_data_item(&bytes, "test data").expect("restore data");
    assert_eq!(restored.as_bytes().unwrap(), vec![0xaa, 0xbb]);

    let err = nep17_payment_data_item(&[0xff], "bad callback data")
        .expect_err("invalid payload should fail");
    assert!(
        err.to_string().contains("bad callback data"),
        "error should preserve call-site context: {err}"
    );
}

#[test]
fn nep17_method_helpers_match_fungible_token_abi_shape() {
    let symbol = super::nep17_symbol_method();
    assert_eq!(symbol.name, "symbol");
    assert_eq!(symbol.cpu_fee, 0);
    assert!(symbol.safe);
    assert_eq!(symbol.required_call_flags, 0);
    assert!(symbol.parameters.is_empty());
    assert_eq!(symbol.return_type, ContractParameterType::String);

    let decimals = super::nep17_decimals_method();
    assert_eq!(decimals.name, "decimals");
    assert_eq!(decimals.cpu_fee, 0);
    assert!(decimals.safe);
    assert_eq!(decimals.required_call_flags, 0);
    assert!(decimals.parameters.is_empty());
    assert_eq!(decimals.return_type, ContractParameterType::Integer);

    let total_supply = super::nep17_total_supply_method(0x04);
    assert_eq!(total_supply.name, "totalSupply");
    assert_eq!(total_supply.cpu_fee, 1 << 15);
    assert!(total_supply.safe);
    assert_eq!(total_supply.required_call_flags, 0x04);
    assert!(total_supply.parameters.is_empty());
    assert_eq!(total_supply.return_type, ContractParameterType::Integer);

    let balance_of = super::nep17_balance_of_method(0x04);
    assert_eq!(balance_of.name, "balanceOf");
    assert_eq!(balance_of.cpu_fee, 1 << 15);
    assert!(balance_of.safe);
    assert_eq!(balance_of.required_call_flags, 0x04);
    assert_eq!(balance_of.parameters, [ContractParameterType::Hash160]);
    assert_eq!(balance_of.parameter_names, ["account"]);
    assert_eq!(balance_of.return_type, ContractParameterType::Integer);

    let transfer = super::nep17_transfer_method();
    assert_eq!(transfer.name, "transfer");
    assert_eq!(transfer.cpu_fee, 1 << 17);
    assert!(!transfer.safe);
    assert_eq!(
        transfer.required_call_flags,
        (neo_primitives::CallFlags::STATES
            | neo_primitives::CallFlags::ALLOW_CALL
            | neo_primitives::CallFlags::ALLOW_NOTIFY)
            .bits()
    );
    assert_eq!(
        transfer.parameters,
        [
            ContractParameterType::Hash160,
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::Any,
        ]
    );
    assert_eq!(transfer.storage_fee, 50);
    assert_eq!(transfer.parameter_names, ["from", "to", "amount", "data"]);
    assert_eq!(transfer.return_type, ContractParameterType::Boolean);
}

#[test]
fn nep_payment_method_helpers_match_callback_abi_shape() {
    let nep17 = super::nep17_payment_method(32, true, 0);
    assert_eq!(nep17.name, NEP17_PAYMENT_METHOD);
    assert_eq!(nep17.cpu_fee, 32);
    assert!(nep17.safe);
    assert_eq!(nep17.required_call_flags, 0);
    assert_eq!(
        nep17.parameters,
        [
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::Any,
        ]
    );
    assert_eq!(nep17.parameter_names, ["from", "amount", "data"]);
    assert_eq!(nep17.return_type, ContractParameterType::Void);

    let nep11 = super::nep11_payment_method(64, false, 0x0f);
    assert_eq!(nep11.name, NEP11_PAYMENT_METHOD);
    assert_eq!(nep11.cpu_fee, 64);
    assert!(!nep11.safe);
    assert_eq!(nep11.required_call_flags, 0x0f);
    assert_eq!(
        nep11.parameters,
        [
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::ByteArray,
            ContractParameterType::Any,
        ]
    );
    assert_eq!(nep11.parameter_names, ["from", "amount", "tokenId", "data"]);
    assert_eq!(nep11.return_type, ContractParameterType::Void);
}

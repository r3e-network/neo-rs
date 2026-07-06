//! # neo-native-contracts::tests::notary
//!
//! Test module grouping Native Notary contract state and request verification
//! behavior. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::storage::DepositState;
use super::*;
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(x), Buffer(y)) => x == y,
        (Array(x), Array(y)) | (Struct(x), Struct(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

#[test]
fn native_contract_surface() {
    let c = Notary::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "getMaxNotValidBeforeDelta",
            "balanceOf",
            "expirationOf",
            "setMaxNotValidBeforeDelta",
            "lockDepositUntil",
            crate::NEP17_PAYMENT_METHOD,
            "withdraw",
            "verify"
        ]
    );
    // verify: ReadStates, (ByteArray) -> Boolean. Manifest-SAFE: C#
    // derives Safe = (ReadStates & ~CallFlags.ReadOnly) == 0
    // (ContractMethodMetadata.cs:74).
    let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
    assert!(verify.safe);
    assert_eq!(verify.required_call_flags, CallFlags::READ_STATES.bits());
    assert_eq!(verify.parameters, vec![ContractParameterType::ByteArray]);
    assert_eq!(verify.return_type, ContractParameterType::Boolean);
    assert_eq!(verify.cpu_fee, 1 << 15);
    // withdraw: not safe, CallFlags.All (re-entrant GAS transfer),
    // (Hash160, Hash160) -> Boolean.
    let withdraw = c.methods().iter().find(|m| m.name == "withdraw").unwrap();
    assert!(!withdraw.safe);
    assert_eq!(withdraw.required_call_flags, CallFlags::ALL.bits());
    assert_eq!(
        withdraw.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Hash160
        ]
    );
    assert_eq!(withdraw.return_type, ContractParameterType::Boolean);
    // onNEP17Payment: not safe, States, (Hash160, Integer, Any) -> Void.
    let on_pay = c
        .methods()
        .iter()
        .find(|m| m.name == crate::NEP17_PAYMENT_METHOD)
        .unwrap();
    assert!(!on_pay.safe);
    assert_eq!(on_pay.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(
        on_pay.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Integer,
            ContractParameterType::Any
        ]
    );
    assert_eq!(on_pay.return_type, ContractParameterType::Void);
    for name in ["balanceOf", "expirationOf"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(m.return_type, ContractParameterType::Integer);
    }
    // The committee-gated setter: not safe, States, Integer -> Void.
    let setter = c
        .methods()
        .iter()
        .find(|m| m.name == "setMaxNotValidBeforeDelta")
        .unwrap();
    assert!(!setter.safe);
    assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(setter.return_type, ContractParameterType::Void);
    assert_eq!(setter.cpu_fee, 1 << 15);
    // lockDepositUntil: not safe, States, (Hash160, Integer) -> Boolean.
    let lock = c
        .methods()
        .iter()
        .find(|m| m.name == "lockDepositUntil")
        .unwrap();
    assert!(!lock.safe);
    assert_eq!(lock.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(
        lock.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Integer
        ]
    );
    assert_eq!(lock.return_type, ContractParameterType::Boolean);
}

#[test]
fn invoke_uint_args_use_shared_raw_parser() {
    let source = include_str!("../../notary/invoke.rs");

    assert!(source.contains("fn invoke_lock_deposit_until("));
    assert!(source.contains("fn invoke_set_max_not_valid_before_delta("));
    assert!(source.contains("crate::args::raw_u32_arg"));
    assert!(!source.contains("BigInt::from_signed_bytes_le(args"));
    assert!(!source.contains("BigInt::from_signed_bytes_le(b)"));
}

#[test]
fn deposit_round_trips_and_lock_decision_matches_csharp() {
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[7u8; 20]).unwrap();

    // No deposit -> read_deposit None; lock decision -> None (false).
    assert!(
        Notary::new()
            .read_deposit(&cache, &account)
            .unwrap()
            .is_none()
    );
    assert!(Notary::lock_deposit_decision(100, None, 200).is_none());

    // Write a deposit (Amount=1000, Till=150) and read it back.
    Notary::new()
        .write_deposit(&cache, &account, &BigInt::from(1000), 150)
        .unwrap();
    let expected = BinarySerializer::serialize(
        &StackItem::from_struct(vec![StackItem::from_int(1000), StackItem::from_int(150)]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        cache
            .get(&Notary::deposit_key(&account))
            .unwrap()
            .value_bytes()
            .as_ref(),
        expected.as_slice()
    );
    assert_eq!(
        Notary::new().read_deposit(&cache, &account).unwrap(),
        Some((BigInt::from(1000), 150))
    );

    let deposit = Notary::new().read_deposit(&cache, &account).unwrap();
    // till below current+2 -> None.
    assert!(Notary::lock_deposit_decision(199, deposit.clone(), 200).is_none());
    // till below existing Till (150) -> None (can't shorten).
    assert!(Notary::lock_deposit_decision(100, deposit.clone(), 149).is_none());
    // Valid extension keeps Amount, updates Till.
    assert_eq!(
        Notary::lock_deposit_decision(100, deposit, 300),
        Some((BigInt::from(1000), 300))
    );

    // The lock write preserves Amount and updates Till.
    Notary::new()
        .write_deposit(&cache, &account, &BigInt::from(1000), 300)
        .unwrap();
    assert_eq!(
        Notary::new().read_deposit(&cache, &account).unwrap(),
        Some((BigInt::from(1000), 300))
    );

    // withdraw's RemoveDepositFor: delete clears the entry.
    Notary::new().delete_deposit(&cache, &account);
    assert!(
        Notary::new()
            .read_deposit(&cache, &account)
            .unwrap()
            .is_none()
    );
}

#[test]
fn deposit_state_interoperable_projection_matches_csharp_shape() {
    let state = DepositState::new(BigInt::from(1000), 42);
    let expected_value = StackValue::Struct(vec![
        StackValue::BigInteger(BigInt::from(1000).to_signed_bytes_le()),
        StackValue::Integer(42),
    ]);

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

    let mut parsed = DepositState::new(BigInt::from(0), 0);
    Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
    assert_eq!(parsed, state);

    assert!(DepositState::from_stack_value(StackValue::Array(vec![])).is_err());
    assert!(
        DepositState::from_stack_value(StackValue::Struct(vec![StackValue::BigInteger(
            BigInt::from(1000).to_signed_bytes_le()
        )]))
        .is_err()
    );
}

#[test]
fn deposit_storage_uses_stack_value_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let source = include_str!("../../notary/storage.rs");
    let writer = slice_between(source, "fn write_deposit(", "fn lock_deposit_decision");
    assert!(writer.contains("encode_storage_struct"));
    assert!(writer.contains("DepositState::new"));
    assert!(!writer.contains("StackValue::Struct"));
    assert!(!writer.contains("StackItem::from_struct"));
    assert!(!writer.contains("BinarySerializer::serialize("));

    let reader = slice_between(source, "fn decode_deposit(", "fn delete_deposit");
    assert!(reader.contains("decode_stack_value"));
    assert!(reader.contains("DepositState::from_stack_value"));
    assert!(!reader.contains("stack_value_as_bigint"));
    assert!(!reader.contains("stack_value_as_u32"));
    assert!(!reader.contains("BinarySerializer::deserialize("));
}

#[test]
fn compute_deposit_matches_csharp_onnep17_rules() {
    let amount = BigInt::from(100);
    // current=10 -> till must be >= 12.
    assert!(Notary::compute_deposit(None, &amount, 11, true, 10, 0).is_err());

    // First deposit below 2*feePerKey (fee=60 -> min 120) -> error.
    assert!(Notary::compute_deposit(None, &amount, 100, true, 10, 60).is_err());
    // First deposit, owner sets till (allowed) -> Amount=amount, Till=requested.
    assert_eq!(
        Notary::compute_deposit(None, &amount, 100, true, 10, 10).unwrap(),
        (BigInt::from(100), 100)
    );
    // First deposit, NOT owner -> till forced to current + DefaultDepositDeltaTill.
    assert_eq!(
        Notary::compute_deposit(None, &amount, 100, false, 10, 10).unwrap(),
        (BigInt::from(100), 10 + DEFAULT_DEPOSIT_DELTA_TILL)
    );

    // Existing deposit: till below previous Till -> error.
    assert!(
        Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 150, true, 10, 0).is_err()
    );
    // Existing, owner extends -> Amount accumulates, Till = requested.
    assert_eq!(
        Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 300, true, 10, 0).unwrap(),
        (BigInt::from(150), 300)
    );
    // Existing, NOT owner -> Amount accumulates, Till unchanged.
    assert_eq!(
        Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 300, false, 10, 0).unwrap(),
        (BigInt::from(150), 200)
    );
}

#[test]
fn parse_onnep17_data_handles_null_and_explicit_to() {
    let from = UInt160::from_bytes(&[1u8; 20]).unwrap();
    let explicit = UInt160::from_bytes(&[2u8; 20]).unwrap();

    // [Null, 500] -> to defaults to `from`.
    let null_to = StackItem::from_array(vec![StackItem::null(), StackItem::from_int(500)]);
    let bytes = BinarySerializer::serialize(&null_to, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        Notary::parse_onnep17_data(&from, &bytes).unwrap(),
        (from, 500)
    );

    // [explicit_to, 700] -> to is the provided hash.
    let with_to = StackItem::from_array(vec![
        StackItem::from_byte_string(explicit.to_bytes()),
        StackItem::from_int(700),
    ]);
    let bytes2 = BinarySerializer::serialize(&with_to, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        Notary::parse_onnep17_data(&from, &bytes2).unwrap(),
        (explicit, 700)
    );

    // Wrong shape (not a 2-element array) -> error.
    let bad = StackItem::from_array(vec![StackItem::from_int(1)]);
    let bad_bytes = BinarySerializer::serialize(&bad, &ExecutionEngineLimits::default()).unwrap();
    assert!(Notary::parse_onnep17_data(&from, &bad_bytes).is_err());

    // C# Notary.OnNEP17Payment (Notary.cs:146-152) only inspects the
    // incoming StackItem's array/null/bytes/integer shape. The Rust parser
    // should use the shared StackValue projection rather than materializing
    // neo_vm::StackItem for this non-VM-inspection path.
    let source = include_str!("../../notary/storage.rs");
    let start = source
        .find("fn parse_onnep17_data")
        .expect("parser source exists");
    let end = source[start..]
        .find("fn compute_deposit")
        .map(|offset| start + offset)
        .expect("next helper marker exists");
    let parser = &source[start..end];
    assert!(parser.contains("decode_stack_value"));
    assert!(parser.contains("StackValue::Array"));
    assert!(parser.contains("StackValue::Null"));
    assert!(!parser.contains("BinarySerializer::deserialize("));
    assert!(!parser.contains("StackItem::Array"));
}

#[test]
fn set_max_not_valid_before_delta_write_round_trips() {
    // The setter's storage effect (overwrite Prefix_MaxNotValidBeforeDelta) is
    // observed by the getMaxNotValidBeforeDelta reader, matching C#
    // GetAndChange(...).Set(value).
    let cache = DataCache::new(false);
    Notary::new().put_max_not_valid_before_delta(&cache, 250);
    assert_eq!(
        cache
            .get(&Notary::max_not_valid_before_delta_key())
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes())),
        Some(BigInt::from(250))
    );
}

#[test]
fn deposit_reads_amount_and_till_or_zero() {
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[3u8; 20]).unwrap();

    // Absent deposit -> both reads are 0.
    assert_eq!(
        Notary::new()
            .read_deposit_field(&cache, &account, 0)
            .unwrap(),
        BigInt::from(0)
    );
    assert_eq!(
        Notary::new()
            .read_deposit_field(&cache, &account, 1)
            .unwrap(),
        BigInt::from(0)
    );

    // Store a Deposit struct [Amount=1000, Till=42] and read each field.
    let deposit = StackItem::from_struct(vec![StackItem::from_int(1000), StackItem::from_int(42)]);
    let bytes = BinarySerializer::serialize(&deposit, &ExecutionEngineLimits::default()).unwrap();
    cache.add(
        Notary::deposit_key(&account),
        StorageItem::from_bytes(bytes),
    );

    assert_eq!(
        Notary::new()
            .read_deposit_field(&cache, &account, 0)
            .unwrap(),
        BigInt::from(1000)
    ); // Amount
    assert_eq!(
        Notary::new()
            .read_deposit_field(&cache, &account, 1)
            .unwrap(),
        BigInt::from(42)
    ); // Till
}

#[test]
fn max_not_valid_before_delta_requires_initialized_storage() {
    let cache = DataCache::new(false);
    let mut engine = ApplicationEngine::new(
        neo_primitives::TriggerType::Application,
        None,
        std::sync::Arc::new(cache),
        None,
        ProtocolSettings::default(),
        0,
        None,
    )
    .expect("engine builds");

    let err = Notary::new()
        .invoke(&mut engine, "getMaxNotValidBeforeDelta", &[])
        .expect_err("missing Notary max delta storage should fault");
    assert!(err.to_string().contains("MaxNotValidBeforeDelta"), "{err}");
}

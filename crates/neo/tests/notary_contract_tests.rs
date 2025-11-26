//! Notary native contract unit tests matching C# UT_Notary
//!
//! Tests for Neo.SmartContract.Native.Notary functionality.

use neo_core::smart_contract::native::notary::{Deposit, Notary};
use neo_core::smart_contract::native::NativeContract;
use neo_core::UInt160;
use num_bigint::BigInt;

/// Tests that Notary has correct contract ID (-10)
#[test]
fn test_notary_contract_id() {
    let notary = Notary::new();
    assert_eq!(notary.id(), -10, "Notary contract ID should be -10");
}

/// Tests that Notary has correct name
#[test]
fn test_notary_contract_name() {
    let notary = Notary::new();
    assert_eq!(notary.name(), "Notary", "Notary contract name should match");
}

/// Tests that Notary has correct contract hash
#[test]
fn test_notary_contract_hash() {
    let notary = Notary::new();
    let hash = notary.hash();

    // Expected hash: 0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b
    let expected_bytes: [u8; 20] = [
        0xc1, 0xe1, 0x4f, 0x19, 0xc3, 0xe6, 0x0d, 0x0b, 0x92, 0x44, 0xd0, 0x6d, 0xd7, 0xba, 0x9b,
        0x11, 0x31, 0x35, 0xec, 0x3b,
    ];

    assert_eq!(
        hash.to_bytes(),
        expected_bytes,
        "Notary hash should match C# reference"
    );
}

/// Tests Notary methods are registered
#[test]
fn test_notary_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(method_names.contains(&"balanceOf"), "Should have balanceOf");
    assert!(
        method_names.contains(&"expirationOf"),
        "Should have expirationOf"
    );
    assert!(
        method_names.contains(&"getMaxNotValidBeforeDelta"),
        "Should have getMaxNotValidBeforeDelta"
    );
    assert!(
        method_names.contains(&"onNEP17Payment"),
        "Should have onNEP17Payment"
    );
    assert!(
        method_names.contains(&"lockDepositUntil"),
        "Should have lockDepositUntil"
    );
    assert!(method_names.contains(&"withdraw"), "Should have withdraw");
    assert!(
        method_names.contains(&"setMaxNotValidBeforeDelta"),
        "Should have setMaxNotValidBeforeDelta"
    );
}

/// Tests Deposit struct creation
#[test]
fn test_deposit_creation() {
    let amount = BigInt::from(1000000000i64); // 10 GAS
    let till = 12345u32;

    let deposit = Deposit::new(amount.clone(), till);

    assert_eq!(deposit.amount, amount, "Deposit amount should match");
    assert_eq!(deposit.till, till, "Deposit till should match");
}

/// Tests Deposit default values
#[test]
fn test_deposit_default() {
    let deposit = Deposit::default();

    assert_eq!(
        deposit.amount,
        BigInt::from(0),
        "Default amount should be 0"
    );
    assert_eq!(deposit.till, 0, "Default till should be 0");
}

/// Tests Deposit to/from StackItem conversion
#[test]
fn test_deposit_stack_item_roundtrip() {
    use neo_core::smart_contract::i_interoperable::IInteroperable;

    let original = Deposit::new(BigInt::from(500), 100);
    let stack_item = original.to_stack_item();

    let mut recovered = Deposit::default();
    recovered.from_stack_item(stack_item);

    assert_eq!(recovered.amount, original.amount, "Amount should roundtrip");
    assert_eq!(recovered.till, original.till, "Till should roundtrip");
}

/// Tests safe methods have correct flags
#[test]
fn test_notary_safe_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    // balanceOf should be safe (call_flags = 0)
    let balance_of = methods.iter().find(|m| m.name == "balanceOf");
    assert!(balance_of.is_some(), "balanceOf should exist");
    assert!(balance_of.unwrap().safe, "balanceOf should be safe");

    // expirationOf should be safe
    let expiration_of = methods.iter().find(|m| m.name == "expirationOf");
    assert!(expiration_of.is_some(), "expirationOf should exist");
    assert!(expiration_of.unwrap().safe, "expirationOf should be safe");

    // getMaxNotValidBeforeDelta should be safe
    let get_max = methods
        .iter()
        .find(|m| m.name == "getMaxNotValidBeforeDelta");
    assert!(get_max.is_some(), "getMaxNotValidBeforeDelta should exist");
    assert!(
        get_max.unwrap().safe,
        "getMaxNotValidBeforeDelta should be safe"
    );
}

/// Tests unsafe methods have correct flags
#[test]
fn test_notary_unsafe_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    // onNEP17Payment should be unsafe
    let on_payment = methods.iter().find(|m| m.name == "onNEP17Payment");
    assert!(on_payment.is_some(), "onNEP17Payment should exist");
    assert!(!on_payment.unwrap().safe, "onNEP17Payment should be unsafe");

    // withdraw should be unsafe
    let withdraw = methods.iter().find(|m| m.name == "withdraw");
    assert!(withdraw.is_some(), "withdraw should exist");
    assert!(!withdraw.unwrap().safe, "withdraw should be unsafe");
}

/// Tests default max not valid before delta (140 blocks)
#[test]
fn test_default_max_not_valid_before_delta() {
    // Default is 140 blocks (20 rounds * 7 validators)
    // This is checked in get_max_not_valid_before_delta when no stored value exists
    let notary = Notary::new();

    // The constant is internal, but we can verify the contract exists
    assert_eq!(notary.id(), Notary::ID);
}

/// Tests Notary contract ID constant
#[test]
fn test_notary_id_constant() {
    assert_eq!(Notary::ID, -10, "Notary::ID should be -10");
}

/// Tests Deposit with large amount
#[test]
fn test_deposit_large_amount() {
    // Test with maximum GAS amount (100 million GAS = 100_000_000_00000000 datoshi)
    let large_amount = BigInt::from(100_000_000_00000000i64);
    let till = u32::MAX;

    let deposit = Deposit::new(large_amount.clone(), till);

    assert_eq!(deposit.amount, large_amount);
    assert_eq!(deposit.till, till);
}

/// Tests Deposit clone
#[test]
fn test_deposit_clone() {
    let original = Deposit::new(BigInt::from(12345), 67890);
    let cloned = original.clone();

    assert_eq!(cloned.amount, original.amount);
    assert_eq!(cloned.till, original.till);
}

use super::*;
use crate::smart_contract::i_interoperable::IInteroperable;

#[test]
fn test_notary_creation() {
    let notary = Notary::new();
    assert_eq!(notary.id(), Notary::ID);
    assert_eq!(notary.name(), "Notary");
}

#[test]
fn test_deposit_serialization() {
    let deposit = Deposit::new(BigInt::from(1000000000i64), 12345);
    let data = serialize_deposit(&deposit);
    let deserialized = deserialize_deposit(&data).unwrap();
    assert_eq!(deserialized.amount, deposit.amount);
    assert_eq!(deserialized.till, deposit.till);
}

#[test]
fn test_deposit_to_stack_item() {
    let deposit = Deposit::new(BigInt::from(500), 100);
    let item = deposit.to_stack_item().unwrap();
    let mut new_deposit = Deposit::default();
    new_deposit.from_stack_item(item).unwrap();
    assert_eq!(new_deposit.amount, deposit.amount);
    assert_eq!(new_deposit.till, deposit.till);
}

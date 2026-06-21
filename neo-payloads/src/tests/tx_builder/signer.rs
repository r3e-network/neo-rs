use super::*;

#[test]
fn default_signer_is_zero_account_none_scope() {
    let s = SignerBuilder::new().build();
    assert_eq!(s.account, UInt160::zero());
    assert_eq!(s.scopes, WitnessScope::NONE);
    assert!(s.allowed_contracts.is_empty());
    assert!(s.allowed_groups.is_empty());
}

#[test]
fn account_and_scope_are_applied() {
    let acct = UInt160::from_bytes(&[7u8; 20]).unwrap();
    let mut b = SignerBuilder::new();
    b.account(acct).scope(WitnessScope::CALLED_BY_ENTRY);
    let s = b.build();
    assert_eq!(s.account, acct);
    assert_eq!(s.scopes, WitnessScope::CALLED_BY_ENTRY);
}

#[test]
fn add_witness_scope_combines_flags() {
    let mut b = SignerBuilder::new();
    b.scope(WitnessScope::CALLED_BY_ENTRY)
        .add_witness_scope(WitnessScope::CUSTOM_CONTRACTS);
    let s = b.build();
    assert!(s.scopes.contains(WitnessScope::CALLED_BY_ENTRY));
    assert!(s.scopes.contains(WitnessScope::CUSTOM_CONTRACTS));
}

#[test]
fn allowed_contracts_are_collected_in_order() {
    let c1 = UInt160::from_bytes(&[1u8; 20]).unwrap();
    let c2 = UInt160::from_bytes(&[2u8; 20]).unwrap();
    let mut b = SignerBuilder::new();
    b.scope(WitnessScope::CUSTOM_CONTRACTS)
        .with_allowed_contract(c1)
        .allow_contract(c2); // alias for with_allowed_contract
    let s = b.build();
    assert_eq!(s.allowed_contracts, vec![c1, c2]);
}

use super::*;

#[test]
fn get_address_uses_provided_address_version() {
    let script_hash = UInt160::from_bytes(&[0x31; UInt160::LENGTH]).unwrap();
    let contract = Contract::create_with_hash(script_hash, Vec::new());

    let version = neo_primitives::constants::ADDRESS_VERSION;
    assert_eq!(contract.get_address(version), script_hash.to_address());
}

#[test]
fn create_contract_caches_script_hash() {
    let script = vec![0x52]; // PUSH2
    let contract = Contract::create(vec![ContractParameterType::Signature], script.clone());
    let hash1 = contract.script_hash();
    let hash2 = contract.script_hash();
    assert_eq!(
        hash1, hash2,
        "script_hash should be cached and deterministic"
    );
}

#[test]
fn create_with_hash_pre_populates_cache() {
    let script_hash = UInt160::from_bytes(&[0x42; UInt160::LENGTH]).unwrap();
    let contract = Contract::create_with_hash(script_hash, Vec::new());
    assert_eq!(
        contract.script_hash(),
        script_hash,
        "pre-supplied hash should be returned"
    );
}

#[test]
fn invalid_multisig_inputs_use_fallible_path_and_fail_closed() {
    assert!(Contract::try_create_multi_sig_contract(1, &[]).is_err());
    assert!(Contract::try_create_multi_sig_redeem_script(1, &[]).is_err());

    let contract = Contract::create_multi_sig_contract(1, &[]);
    assert!(contract.script.is_empty());
    assert!(contract.parameter_list.is_empty());
    assert!(Contract::create_multi_sig_redeem_script(1, &[]).is_empty());
}

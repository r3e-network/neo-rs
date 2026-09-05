use neo_core::UInt160;
use neo_core::smart_contract::manifest::ContractPermissionDescriptor;
use neo_core::wallets::KeyPair;

#[test]
fn contract_permission_descriptor_equality_matches_csharp() {
    let wildcard = ContractPermissionDescriptor::create_wildcard();
    let wildcard_again = ContractPermissionDescriptor::create_wildcard();
    assert_eq!(wildcard, wildcard_again);

    let hash_descriptor = ContractPermissionDescriptor::create_hash(UInt160::zero());
    assert_ne!(wildcard, hash_descriptor);

    let key_pair = KeyPair::new(vec![1u8; 32]).expect("keypair");
    let group_key = key_pair.get_public_key_point().expect("pubkey");
    let group_descriptor = ContractPermissionDescriptor::create_group(group_key);

    assert_ne!(wildcard, group_descriptor);
    assert_eq!(group_descriptor, group_descriptor.clone());
}

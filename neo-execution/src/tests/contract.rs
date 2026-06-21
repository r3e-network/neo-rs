use super::*;

#[test]
fn get_address_uses_default_protocol_base58_check_payload() {
    let script_hash = UInt160::from_bytes(&[0x31; UInt160::LENGTH]).unwrap();
    let contract = Contract::create_with_hash(script_hash, Vec::new());

    assert_eq!(contract.get_address(), script_hash.to_address());
}

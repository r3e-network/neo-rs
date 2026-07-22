use super::*;
use crate::types::test_fixtures::rpc_case_result;

#[test]
fn validate_address_roundtrip() {
    let result = RpcValidateAddressResult {
        address: "addr".to_string(),
        is_valid: true,
    };
    let json = result.to_json();
    let parsed = RpcValidateAddressResult::from_json(&json).unwrap();
    assert_eq!(parsed.address, result.address);
    assert!(parsed.is_valid);
}

#[test]
fn validate_address_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("validateaddressasync") else {
        return;
    };
    let parsed = RpcValidateAddressResult::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

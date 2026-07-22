use super::*;
use crate::types::test_fixtures::rpc_case_result;

#[test]
fn rpc_unclaimed_gas_roundtrip() {
    let gas = RpcUnclaimedGas {
        unclaimed: 1234,
        address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
    };
    let json = gas.to_json();
    let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
    assert_eq!(parsed.unclaimed, gas.unclaimed);
    assert_eq!(parsed.address, gas.address);
}

#[test]
fn rpc_unclaimed_gas_rejects_invalid_value() {
    let mut json = JObject::new();
    json.insert(
        "unclaimed".to_string(),
        JToken::String("not-a-number".into()),
    );
    json.insert(
        "address".to_string(),
        JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
    );
    assert!(RpcUnclaimedGas::from_json(&json).is_err());
}

#[test]
fn rpc_unclaimed_gas_accepts_numeric_value() {
    let mut json = JObject::new();
    json.insert("unclaimed".to_string(), JToken::Number(5f64));
    json.insert(
        "address".to_string(),
        JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
    );
    let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
    assert_eq!(parsed.unclaimed, 5);
}

#[test]
fn unclaimed_gas_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getunclaimedgasasync") else {
        return;
    };
    let parsed = RpcUnclaimedGas::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

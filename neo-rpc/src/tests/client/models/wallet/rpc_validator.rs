use super::*;
use crate::types::test_fixtures::rpc_case_result_array;
use neo_serialization::json::JArray;

#[test]
fn rpc_validator_roundtrip() {
    let validator = RpcValidator {
        public_key: "03abcdef".to_string(),
        votes: BigInt::from(123_456u64),
    };
    let json = validator.to_json();
    let parsed = RpcValidator::from_json(&json).expect("validator");
    assert_eq!(parsed.public_key, validator.public_key);
    assert_eq!(parsed.votes, validator.votes);
}

#[test]
fn rpc_validator_rejects_invalid_votes() {
    let mut json = JObject::new();
    json.insert("publickey".to_string(), JToken::String("03abcdef".into()));
    json.insert("votes".to_string(), JToken::String("not-a-number".into()));

    assert!(RpcValidator::from_json(&json).is_err());
}

#[test]
fn rpc_validator_accepts_numeric_votes() {
    let mut json = JObject::new();
    json.insert("publickey".to_string(), JToken::String("03abcdef".into()));
    json.insert("votes".to_string(), JToken::Number(5f64));

    let parsed = RpcValidator::from_json(&json).expect("validator");
    assert_eq!(parsed.votes, BigInt::from(5));
}

#[test]
fn validators_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result_array("getnextblockvalidatorsasync") else {
        return;
    };
    let parsed = expected
        .children()
        .iter()
        .filter_map(|entry| entry.as_ref())
        .filter_map(|token| token.as_object())
        .filter_map(|obj| RpcValidator::from_json(obj).ok())
        .collect::<Vec<_>>();
    let actual = JArray::from(
        parsed
            .iter()
            .map(|validator| JToken::Object(validator.to_json()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(expected.to_string(), actual.to_string());
}

//! Tests for the transport-neutral raw-mempool RPC model.

use super::*;
use crate::types::test_fixtures::rpc_case_result;
use neo_serialization::json::JArray;
use neo_serialization::json::JToken;

#[test]
fn raw_mempool_roundtrip() {
    let pool = RpcRawMemPool {
        height: 10,
        verified: vec![UInt256::zero()],
        unverified: vec![UInt256::zero()],
    };
    let json = pool.to_json();
    let parsed = RpcRawMemPool::from_json(&json).unwrap();
    assert_eq!(parsed.height, pool.height);
    assert_eq!(parsed.verified.len(), 1);
    assert_eq!(parsed.unverified.len(), 1);
}

#[test]
fn raw_mempool_accepts_numeric_height() {
    let mut json = JObject::new();
    json.insert("height".to_string(), JToken::Number(5f64));
    json.insert("verified".to_string(), JToken::Array(JArray::new()));
    json.insert("unverified".to_string(), JToken::Array(JArray::new()));

    let parsed = RpcRawMemPool::from_json(&json).unwrap();
    assert_eq!(parsed.height, 5);
}

#[test]
fn raw_mempool_accepts_string_height() {
    let mut json = JObject::new();
    json.insert("height".to_string(), JToken::String("7".to_string()));
    json.insert("verified".to_string(), JToken::Array(JArray::new()));
    json.insert("unverified".to_string(), JToken::Array(JArray::new()));

    let parsed = RpcRawMemPool::from_json(&json).unwrap();
    assert_eq!(parsed.height, 7);
}

#[test]
fn raw_mempool_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getrawmempoolbothasync") else {
        return;
    };
    let parsed = RpcRawMemPool::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

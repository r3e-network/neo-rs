use super::*;
use base64::{Engine as _, engine::general_purpose};
use neo_serialization::json::JArray;

#[test]
fn rpc_state_root_parses_with_witness() {
    let mut json = JObject::new();
    json.insert(
        "version".to_string(),
        neo_serialization::json::JToken::Number(0f64),
    );
    json.insert(
        "index".to_string(),
        neo_serialization::json::JToken::Number(1f64),
    );
    json.insert(
        "roothash".to_string(),
        neo_serialization::json::JToken::String(UInt256::zero().to_string()),
    );

    let mut witness_obj = JObject::new();
    witness_obj.insert(
        "invocation".to_string(),
        neo_serialization::json::JToken::String(general_purpose::STANDARD.encode(b"i")),
    );
    witness_obj.insert(
        "verification".to_string(),
        neo_serialization::json::JToken::String(general_purpose::STANDARD.encode(b"v")),
    );
    json.insert(
        "witnesses".to_string(),
        neo_serialization::json::JToken::Array(JArray::from(vec![
            neo_serialization::json::JToken::Object(witness_obj),
        ])),
    );

    let parsed = RpcStateRoot::from_json(&json).expect("state root");
    assert_eq!(parsed.version, 0);
    assert_eq!(parsed.index, 1);
    assert_eq!(parsed.root_hash, UInt256::zero());
    let witness = parsed.witness.expect("witness");
    assert_eq!(witness.invocation_script(), b"i");
    assert_eq!(witness.verification_script(), b"v");
}

#[test]
fn rpc_state_root_allows_missing_witness() {
    let mut json = JObject::new();
    json.insert(
        "version".to_string(),
        neo_serialization::json::JToken::Number(0f64),
    );
    json.insert(
        "index".to_string(),
        neo_serialization::json::JToken::Number(1f64),
    );
    json.insert(
        "roothash".to_string(),
        neo_serialization::json::JToken::String(UInt256::zero().to_string()),
    );

    let parsed = RpcStateRoot::from_json(&json).expect("state root");
    assert!(parsed.witness.is_none());
}

#[test]
fn rpc_state_root_roundtrip() {
    let root = RpcStateRoot {
        version: 1,
        index: 10,
        root_hash: UInt256::zero(),
        witness: None,
    };
    let json = root.to_json();
    let parsed = RpcStateRoot::from_json(&json).expect("state root");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.index, 10);
}

#[test]
fn rpc_state_root_roundtrip_with_witness_json() {
    let witness = super::super::super::utility::witness_from_json(&{
        let mut obj = JObject::new();
        obj.insert(
            "invocation".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"i")),
        );
        obj.insert(
            "verification".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"v")),
        );
        obj
    })
    .unwrap();

    let root = RpcStateRoot {
        version: 2,
        index: 11,
        root_hash: UInt256::zero(),
        witness: Some(witness),
    };
    let json = root.to_json();
    let parsed = RpcStateRoot::from_json(&json).expect("state root");
    assert!(parsed.witness.is_some());
    let parsed_witness = parsed.witness.unwrap();
    assert_eq!(parsed_witness.invocation_script(), b"i");
    assert_eq!(parsed_witness.verification_script(), b"v");
}

use super::super::test_fixtures::rpc_case_result;
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use neo_serialization::json::{JArray, JToken};

fn sample_witness_json(invocation: &[u8], verification: &[u8]) -> JObject {
    let mut obj = JObject::new();
    obj.insert(
        "invocation".to_string(),
        JToken::String(BASE64.encode(invocation)),
    );
    obj.insert(
        "verification".to_string(),
        JToken::String(BASE64.encode(verification)),
    );
    obj
}

#[test]
fn parses_block_header_from_json() {
    let mut json = JObject::new();
    json.insert("version".to_string(), JToken::Number(0.0));
    json.insert(
        "previousblockhash".to_string(),
        JToken::String(UInt256::zero().to_string()),
    );
    json.insert(
        "merkleroot".to_string(),
        JToken::String(UInt256::zero().to_string()),
    );
    json.insert("time".to_string(), JToken::Number(123.0));
    json.insert(
        "nonce".to_string(),
        JToken::String(format!("0X{:016x}", 42u64)),
    );
    json.insert("index".to_string(), JToken::Number(5.0));
    json.insert("primary".to_string(), JToken::Number(3.0));
    json.insert(
        "nextconsensus".to_string(),
        JToken::String(neo_primitives::UInt160::zero().to_string()),
    );

    let witness_json = sample_witness_json(&[1, 2, 3], &[4, 5, 6]);
    json.insert(
        "witnesses".to_string(),
        JToken::Array(JArray::from(vec![JToken::Object(witness_json)])),
    );

    json.insert("confirmations".to_string(), JToken::Number(8.0));
    json.insert(
        "nextblockhash".to_string(),
        JToken::String(UInt256::zero().to_string()),
    );

    let settings = ProtocolSettings::default();
    let rpc_header = RpcBlockHeader::from_json(&json, &settings).expect("should parse");

    assert_eq!(rpc_header.header.version(), 0);
    assert_eq!(rpc_header.header.timestamp(), 123);
    assert_eq!(rpc_header.header.nonce(), 42);
    assert_eq!(rpc_header.confirmations, 8);
    assert!(rpc_header.next_block_hash.is_some());
    // header carries exactly one witness by type (single `witness` field)
}

#[test]
fn block_header_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getblockheaderasync") else {
        return;
    };
    let settings = ProtocolSettings::default_settings();
    let parsed = RpcBlockHeader::from_json(&expected, &settings).expect("parse");
    let actual = parsed.to_json(&settings);
    assert_eq!(expected.to_string(), actual.to_string());
}

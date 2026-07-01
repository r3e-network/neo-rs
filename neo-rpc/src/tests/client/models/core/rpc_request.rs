use super::super::test_fixtures::rpc_case_request;
use super::*;
use neo_serialization::json::JArray;

#[test]
fn rpc_request_roundtrip() {
    let req = RpcRequest {
        id: JToken::Number(7f64),
        json_rpc: "2.0".to_string(),
        method: "getblock".to_string(),
        params: vec![JToken::String("0xabc".to_string())],
    };
    let json = req.to_json();
    let parsed = RpcRequest::from_json(&json).unwrap();
    assert_eq!(parsed.id.as_number(), Some(7f64));
    assert_eq!(parsed.json_rpc, req.json_rpc);
    assert_eq!(parsed.method, req.method);
    assert_eq!(parsed.params.len(), 1);
}

#[test]
fn rpc_request_defaults_params_and_accepts_string_id() {
    let mut json = JObject::new();
    json.insert("id".to_string(), JToken::String("abc".to_string()));
    json.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
    json.insert(
        "method".to_string(),
        JToken::String("getversion".to_string()),
    );

    let parsed = RpcRequest::from_json(&json).unwrap();
    assert_eq!(parsed.id.as_string().unwrap(), "abc");
    assert!(parsed.params.is_empty());
}

fn build_expected_request(request: &JObject) -> JObject {
    let mut expected = JObject::new();
    expected.insert(
        "id".to_string(),
        request.get("id").cloned().unwrap_or(JToken::Null),
    );
    expected.insert(
        "jsonrpc".to_string(),
        request
            .get("jsonrpc")
            .cloned()
            .unwrap_or(JToken::String("2.0".into())),
    );
    expected.insert(
        "method".to_string(),
        request
            .get("method")
            .cloned()
            .unwrap_or(JToken::String(String::new())),
    );
    expected.insert(
        "params".to_string(),
        request
            .get("params")
            .cloned()
            .unwrap_or(JToken::Array(JArray::new())),
    );
    expected
}

#[test]
fn request_to_json_matches_rpc_test_case_with_params() {
    let Some(request) = rpc_case_request("sendrawtransactionasyncerror") else {
        return;
    };
    let expected = build_expected_request(&request);
    let parsed = RpcRequest::from_json(&request).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

#[test]
fn request_to_json_matches_rpc_test_case_without_params() {
    let Some(request) = rpc_case_request("getbestblockhashasync") else {
        return;
    };
    let expected = build_expected_request(&request);
    let parsed = RpcRequest::from_json(&request).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

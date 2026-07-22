use super::*;
use crate::types::test_fixtures::rpc_case_response;
use neo_serialization::json::JToken;

#[test]
fn rpc_response_roundtrip_success() {
    let resp = RpcResponse {
        id: JToken::Number(1f64),
        json_rpc: "2.0".to_string(),
        error: None,
        result: Some(JToken::String("ok".to_string())),
        raw_response: None,
    };
    let json = resp.to_json();
    let parsed = RpcResponse::from_json(&json).unwrap();
    assert_eq!(parsed.json_rpc, resp.json_rpc);
    assert!(parsed.error.is_none());
    assert_eq!(parsed.result.as_ref().unwrap().as_string().unwrap(), "ok");
}

#[test]
fn rpc_response_roundtrip_error() {
    let err = RpcResponseError {
        code: -1,
        message: "bad".to_string(),
        data: Some(JToken::String("info".to_string())),
    };
    let mut json = JObject::new();
    json.insert("id".to_string(), JToken::Null);
    json.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
    json.insert("error".to_string(), JToken::Object(err.to_json()));

    let parsed = RpcResponse::from_json(&json).unwrap();
    let parsed_err = parsed.error.unwrap();
    assert_eq!(parsed_err.code, err.code);
    assert_eq!(parsed_err.message, err.message);
    assert_eq!(parsed_err.data.unwrap().as_string().unwrap(), "info");
}

#[test]
fn rpc_response_to_json_with_result_and_error_data() {
    let resp = RpcResponse {
        id: JToken::String("1".into()),
        json_rpc: "2.0".into(),
        error: Some(RpcResponseError {
            code: -32000,
            message: "failure".into(),
            data: Some(JToken::String("details".into())),
        }),
        result: Some(JToken::String("ignored".into())),
        raw_response: None,
    };

    let json = resp.to_json();
    let parsed = RpcResponse::from_json(&json).unwrap();
    let err = parsed.error.unwrap();
    assert_eq!(err.code, -32000);
    assert_eq!(err.message, "failure");
    assert_eq!(err.data.unwrap().as_string().unwrap(), "details");
    assert_eq!(parsed.result.unwrap().as_string().unwrap(), "ignored");
}

fn build_expected_response(response: &JObject) -> JObject {
    let mut expected = JObject::new();
    expected.insert(
        "id".to_string(),
        response.get("id").cloned().unwrap_or(JToken::Null),
    );
    expected.insert(
        "jsonrpc".to_string(),
        response
            .get("jsonrpc")
            .cloned()
            .unwrap_or(JToken::String("2.0".into())),
    );
    expected.insert(
        "error".to_string(),
        response.get("error").cloned().unwrap_or(JToken::Null),
    );
    expected.insert(
        "result".to_string(),
        response.get("result").cloned().unwrap_or(JToken::Null),
    );
    expected
}

#[test]
fn response_to_json_matches_rpc_test_case_success() {
    let Some(response) = rpc_case_response("getbestblockhashasync") else {
        return;
    };
    let expected = build_expected_response(&response);
    let parsed = RpcResponse::from_json(&response).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

#[test]
fn response_to_json_matches_rpc_test_case_error() {
    let Some(response) = rpc_case_response("sendrawtransactionasyncerror") else {
        return;
    };
    let expected = build_expected_response(&response);
    let parsed = RpcResponse::from_json(&response).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

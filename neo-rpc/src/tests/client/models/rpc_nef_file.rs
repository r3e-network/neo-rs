use super::super::test_fixtures::rpc_case_result;
use super::*;
use neo_manifest::MethodToken;

fn sample_nef() -> NefFile {
    NefFile {
        compiler: "neo".into(),
        source: "src".into(),
        tokens: vec![MethodToken::default()],
        script: vec![1, 2, 3],
        checksum: 999,
    }
}

#[test]
fn rpc_nef_file_roundtrip() {
    let nef = sample_nef();
    let rpc = RpcNefFile::new(nef.clone());
    let json = rpc.to_json();
    let parsed = RpcNefFile::from_json(&json).expect("nef");
    assert_eq!(parsed.nef_file.compiler, nef.compiler);
    assert_eq!(parsed.nef_file.tokens.len(), nef.tokens.len());
    assert_eq!(parsed.nef_file.script, nef.script);
    assert_eq!(parsed.nef_file.checksum, nef.checksum);
}

#[test]
fn rpc_nef_file_rejects_missing_script() {
    let mut json = JObject::new();
    json.insert("compiler".to_string(), JToken::String("neo".into()));
    json.insert("source".to_string(), JToken::String("src".into()));
    json.insert(
        "tokens".to_string(),
        JToken::Array(neo_serialization::json::JArray::new()),
    );
    json.insert("checksum".to_string(), JToken::Number(1f64));

    assert!(RpcNefFile::from_json(&json).is_err());
}

#[test]
fn nef_to_json_matches_rpc_test_case() {
    let Some(result) = rpc_case_result("getcontractstateasync") else {
        return;
    };
    let expected = result
        .get("nef")
        .and_then(JToken::as_object)
        .expect("nef result");
    let parsed = RpcNefFile::from_json(expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}

use super::*;

#[test]
fn rpc_method_invocation_defaults_parameters() {
    let invocation = RpcMethodInvocation {
        script: "00c56b".into(),
        parameters: Vec::new(),
    };
    let json = serde_json::to_string(&invocation).expect("serialize");
    let parsed: RpcMethodInvocation = serde_json::from_str(&json).expect("deserialize");
    assert!(parsed.parameters.is_empty());
    assert_eq!(parsed.script, "00c56b");
}

#[test]
fn rpc_method_invocation_parses_parameters() {
    let json =
        r#"{"script": "00c56b", "parameters": [ {"type": "String", "value": "hello"} ]}"#;
    let parsed: RpcMethodInvocation = serde_json::from_str(json).expect("deserialize");
    assert_eq!(parsed.parameters.len(), 1);
}

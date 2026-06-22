use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_returns_unknown_contract_for_missing_contract() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let unknown = UInt160::zero().to_string();
    let params = [Value::String(unknown)];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("should error");
    assert_eq!(err.code(), -102);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_rejects_invalid_hash() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [Value::String("invalid_script_hash".to_string())];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("invalid hash");
    assert_invalid_params_data(&err, "invalid script hash: Invalid format: Invalid format");
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_returns_true_for_deployed_contract() {
    let server = make_server(RpcServerConfig::default());
    let contract_hash = deploy_verify_contract(&server.system());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [Value::String(contract_hash.to_string())];
    let result = (invokecontractverify.callback())(&server, &params).expect("invoke verify");
    assert_eq!(result.get("state").and_then(Value::as_str), Some("HALT"));

    let stack = result
        .get("stack")
        .and_then(Value::as_array)
        .expect("stack");
    let first = stack.first().expect("stack item");
    assert_eq!(first.get("type").and_then(Value::as_str), Some("Boolean"));
    assert_eq!(first.get("value").and_then(Value::as_bool), Some(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_rejects_missing_verify_overload() {
    let server = make_server(RpcServerConfig::default());
    let contract_hash = deploy_verify_contract(&server.system());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let params = [
        Value::String(contract_hash.to_string()),
        json!([{"type": "Integer", "value": "0"}]),
    ];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("missing overload");
    assert_eq!(err.code(), -512);
}

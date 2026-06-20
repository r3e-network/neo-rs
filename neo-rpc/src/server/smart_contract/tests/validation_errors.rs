use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_script_hash() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String("0x1234".to_string()),
        Value::String("symbol".to_string()),
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid hash");
    assert_invalid_params_data(&err, "invalid script hash: Invalid format: Invalid format");
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_string_params_with_stable_messages() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let err = (invokefunction.callback())(&server, &[]).expect_err("missing script hash");
    assert_invalid_params_data(&err, "invokefunction expects string parameter 1");

    let params = [
        Value::String(UInt160::zero().to_string()),
        Value::Number(serde_json::Number::from(1)),
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("operation type");
    assert_invalid_params_data(&err, "invokefunction expects string parameter 2");
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_signer_scope() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
         "signer": {
             "account": UInt160::zero().to_string(),
             "scopes": "InvalidScopeValue"
        }
    }]);
    let params = [
        Value::String(UInt160::zero().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid scopes");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_signer_account() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
         "signer": {
             "account": "NotAValidHash160",
             "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid account");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_witness_invocation() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
         "signer": {
             "account": UInt160::zero().to_string(),
             "scopes": "CalledByEntry"
        },
         "witness": {
             "invocation": "!@#$",
             "verification": BASE64_STANDARD.encode([0x01])
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid invocation");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_witness_verification() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let signers = json!([{
         "signer": {
             "account": UInt160::zero().to_string(),
             "scopes": "CalledByEntry"
        },
         "witness": {
             "invocation": BASE64_STANDARD.encode([0x01]),
             "verification": "!@#$"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid verification");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_rejects_invalid_contract_parameter() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String(UInt160::zero().to_string()),
        Value::String("transfer".to_string()),
        json!([
            {"type": "Integer", "value": "NotAnInteger"}
        ]),
    ];
    let err = (invokefunction.callback())(&server, &params).expect_err("invalid parameter");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_returns_fault_state_for_missing_method() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("nonExistentMethod".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");

    let state = result.get("state").and_then(Value::as_str).expect("state");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception");
    assert!(exception.contains("doesn't exist"));
}

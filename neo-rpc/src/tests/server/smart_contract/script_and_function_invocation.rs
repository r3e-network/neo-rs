use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_returns_fault_state_in_result() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = vec![OpCode::ABORT.byte()];
    let params = [Value::String(BASE64_STANDARD.encode(script))];
    let result = (invokescript.callback())(&server, &params).expect("invoke result");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state field");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception field");
    assert!(
        exception.contains("ABORT"),
        "expected ABORT message, got {exception}"
    );
}
#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_total_supply_matches_csharp() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let neo_hash = NeoToken::new().hash().to_string();
    let params = [
        Value::String(neo_hash),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke totalSupply");

    let script = result
        .get("script")
        .and_then(|value| value.as_str())
        .expect("script");
    assert_eq!(
        script,
        "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS"
    );

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Integer"));
    assert_eq!(
        first.get("value").and_then(|v| v.as_str()),
        Some("100000000")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_symbol_returns_byte_string() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let neo_hash = NeoToken::new().hash().to_string();
    let params = [
        Value::String(neo_hash),
        Value::String("symbol".to_string()),
        Value::Array(Vec::new()),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke symbol");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(
        first.get("type").and_then(|v| v.as_str()),
        Some("ByteString")
    );
    assert_eq!(first.get("value").and_then(|v| v.as_str()), Some("TkVP"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_cryptolib_sha256_with_argument_halts() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let crypto_hash = CryptoLib::new().hash().to_string();
    let params = [
        Value::String(crypto_hash),
        Value::String("sha256".to_string()),
        // ByteArray param values are base64 (C# Convert.FromBase64String); "aGVsbG8=" == b"hello".
        json!([{"type": "ByteArray", "value": "aGVsbG8="}]),
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke cryptolib sha256");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(
        first.get("type").and_then(|v| v.as_str()),
        Some("ByteString")
    );
    let value = first
        .get("value")
        .and_then(|v| v.as_str())
        .expect("byte string value");
    assert!(!value.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_total_supply_matches_csharp() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS";
    let params = [Value::String(script.to_string())];
    let result = (invokescript.callback())(&server, &params).expect("invoke script");

    let echoed = result
        .get("script")
        .and_then(|value| value.as_str())
        .expect("script");
    assert_eq!(echoed, script);

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Integer"));
    assert_eq!(
        first.get("value").and_then(|v| v.as_str()),
        Some("100000000")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_push1_reports_csharp_gas_units() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let params = [Value::String("EQ==".to_string())];
    let result = (invokescript.callback())(&server, &params).expect("invoke push1 script");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let gas = result
        .get("gasconsumed")
        .and_then(|value| value.as_str())
        .expect("gasconsumed");
    // PUSH1 feeUnits=1 * default ExecFeeFactor=30 = 30 datoshi (matches C#).
    // Test pre-dated commit 4f599eb2 which corrected the 30× cpu_fee undercharge.
    assert_eq!(gas, "30");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Integer"));
    assert_eq!(first.get("value").and_then(|v| v.as_str()), Some("1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_transfer_returns_false() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "CxEMFPlu76Cuc+bgteStE4ozsOWTNUdrDBQtYNweHko3YcnMFOes3ceblcI/lRTAHwwIdHJhbnNmZXIMFPVj6kC8KD1NDgXEjqMFs/Kgc0DvQWJ9W1I=";
    let params = [Value::String(script.to_string())];
    let result = (invokescript.callback())(&server, &params).expect("invoke transfer script");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state");
    assert_eq!(state, "HALT");

    let stack = result
        .get("stack")
        .and_then(|value| value.as_array())
        .expect("stack");
    let first = stack.first().expect("stack entry");
    assert_eq!(first.get("type").and_then(|v| v.as_str()), Some("Boolean"));
    assert_eq!(first.get("value").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_with_diagnostics_reports_invoked_contract_and_storage_shape() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = "wh8MC3RvdGFsU3VwcGx5DBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtS";
    let params = [
        Value::String(script.to_string()),
        Value::Array(Vec::new()),
        Value::Bool(true),
    ];
    let result = (invokescript.callback())(&server, &params).expect("invoke script");

    let diagnostics = result
        .get("diagnostics")
        .and_then(Value::as_object)
        .expect("diagnostics");
    let invoked = diagnostics
        .get("invokedcontracts")
        .expect("invokedcontracts");

    fn collect_hashes(node: &Value, output: &mut Vec<String>) {
        let Some(obj) = node.as_object() else {
            return;
        };
        if let Some(hash) = obj.get("hash").and_then(Value::as_str) {
            output.push(hash.to_string());
        }
        if let Some(children) = obj.get("call").and_then(Value::as_array) {
            for child in children {
                collect_hashes(child, output);
            }
        }
    }

    let mut hashes = Vec::new();
    collect_hashes(invoked, &mut hashes);
    assert!(hashes.contains(&NeoToken::new().hash().to_string()));

    let storage_changes = diagnostics
        .get("storagechanges")
        .and_then(Value::as_array)
        .expect("storagechanges");
    assert!(storage_changes.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_rejects_invalid_base64() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let params = [Value::String("not-base64".to_string())];
    let err = (invokescript.callback())(&server, &params).expect_err("invalid base64");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_faults_when_gas_limit_exceeded() {
    let config = RpcServerConfig {
        max_gas_invoke: 1_000_000,
        ..Default::default()
    };
    let max_gas = config.max_gas_invoke;
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let mut builder = neo_vm::script_builder::ScriptBuilder::new();
    builder.emit_jump(OpCode::JMP_L, 0).expect("jump loop");
    let script = builder.to_array();

    let params = [Value::String(BASE64_STANDARD.encode(script))];
    let result = (invokescript.callback())(&server, &params).expect("invoke loop");

    let state = result.get("state").and_then(Value::as_str).expect("state");
    assert_eq!(state, "FAULT");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        exception.to_ascii_lowercase().contains("insufficient gas"),
        "expected insufficient gas error, got: {exception}"
    );

    let gas_consumed = result
        .get("gasconsumed")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("gasconsumed");
    assert!(gas_consumed >= max_gas);
}

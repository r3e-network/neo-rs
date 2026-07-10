use super::*;

#[tokio::test]
async fn request_hooks_fire_on_successful_call() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":7}"#)
        .create();

    let calls = Arc::new(AtomicUsize::new(0));
    let successful = Arc::new(AtomicBool::new(false));
    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url)
        .with_observer({
            let calls = Arc::clone(&calls);
            let successful = Arc::clone(&successful);
            move |outcome: &RpcRequestOutcome| {
                calls.fetch_add(1, Ordering::SeqCst);
                successful.store(outcome.success, Ordering::SeqCst);
                assert_eq!(outcome.method, "getblockcount");
                assert!(outcome.elapsed > Duration::from_millis(0));
            }
        })
        .build()
        .unwrap();
    let count = client.get_block_count().await.expect("block count");
    assert_eq!(count, 7);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(successful.load(Ordering::SeqCst));
}

#[tokio::test]
async fn request_hooks_fire_on_failed_call() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"boom"}}"#)
        .create();

    let calls = Arc::new(AtomicUsize::new(0));
    let successful = Arc::new(AtomicBool::new(false));
    let hooks = RpcClientHooks::new().with_observer({
        let calls = Arc::clone(&calls);
        let successful = Arc::clone(&successful);
        move |outcome: &RpcRequestOutcome| {
            calls.fetch_add(1, Ordering::SeqCst);
            successful.store(outcome.success, Ordering::SeqCst);
            assert_eq!(outcome.method, "getblockcount");
            assert!(!outcome.success);
            assert_eq!(outcome.error_code, Some(-1));
        }
    });

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).hooks(hooks).build().unwrap();
    let result = client.get_block_count().await;
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!successful.load(Ordering::SeqCst));
}

#[tokio::test]
async fn rpc_client_respects_timeout_and_notifies_hooks() {
    if !localhost_binding_permitted() {
        return;
    }
    let (addr, shutdown_tx) = start_slow_server(
        Duration::from_millis(200),
        r#"{"jsonrpc":"2.0","id":1,"result":1}"#,
    )
    .await;
    let url = Url::parse(&format!("http://{}", addr)).unwrap();

    let notified = Arc::new(AtomicBool::new(false));
    let hooks = RpcClientHooks::new().with_observer({
        let notified = Arc::clone(&notified);
        move |outcome: &RpcRequestOutcome| {
            notified.store(true, Ordering::SeqCst);
            assert_eq!(outcome.method, "slowcall");
            assert!(!outcome.success);
            assert!(outcome.elapsed >= Duration::from_millis(50));
            assert!(outcome.error_code.is_some());
        }
    });

    let client = RpcClient::builder(url)
        .timeout(Duration::from_millis(50))
        .hooks(hooks)
        .build()
        .unwrap();

    let result = client.rpc_send_async("slowcall", vec![]).await;
    assert!(result.is_err());
    assert!(notified.load(Ordering::SeqCst));
    let _ = shutdown_tx.send(());
}

#[test]
fn parse_plugins_supports_category() {
    let mut plugin_obj = JObject::new();
    plugin_obj.insert("name".to_string(), JToken::String("RpcServer".into()));
    plugin_obj.insert("version".to_string(), JToken::String("1.2.3".into()));
    plugin_obj.insert("category".to_string(), JToken::String("Rpc".into()));
    plugin_obj.insert(
        "interfaces".to_string(),
        JToken::Array(JArray::from(vec![JToken::String("IBlock".into())])),
    );

    let array = JToken::Array(JArray::from(vec![JToken::Object(plugin_obj)]));
    let parsed = helpers::parse_plugins(&array).expect("parse plugins");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "RpcServer");
    assert_eq!(parsed[0].version, "1.2.3");
    assert_eq!(parsed[0].category.as_deref(), Some("Rpc"));
    assert_eq!(parsed[0].interfaces, vec!["IBlock".to_string()]);
}

#[test]
fn rpc_client_new_constructs_and_drops() {
    let url = Url::parse("http://www.xxx.yyy").expect("url");
    let client = RpcClient::new(url, None, None, None).expect("client");
    drop(client);
}

#[test]
fn default_rpc_client_hooks_are_zero_sized() {
    assert_eq!(std::mem::size_of::<TracingRpcObserver>(), 0);
    assert_eq!(std::mem::size_of::<RpcClientHooks>(), 0);
}

#[test]
fn observer_closures_are_stored_inline() {
    fn assert_inline_storage<O>(_: &RpcClientHooks<O>)
    where
        O: RpcObserver,
    {
        assert_eq!(
            std::mem::size_of::<RpcClientHooks<O>>(),
            std::mem::size_of::<O>()
        );
        assert!(std::mem::size_of::<O>() > std::mem::size_of::<usize>() * 2);
    }

    let payload = [7_u8; 64];
    let hooks = RpcClientHooks::new().with_observer(move |outcome: &RpcRequestOutcome| {
        let payload = std::hint::black_box(payload);
        std::hint::black_box(outcome.success);
        std::hint::black_box(payload);
    });

    assert_inline_storage(&hooks);
}

#[tokio::test]
async fn rpc_send_by_hash_or_index_parses_negative_index() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*-1\s*\]"#;
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"deadbeef"}"#)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let result = client
        .get_block_header_hex("-1")
        .await
        .expect("block header hex");
    assert_eq!(result, "deadbeef");
}

#[tokio::test]
async fn rpc_send_by_hash_or_index_trims_numeric_input() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*7\s*\]"#;
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":"beef"}"#)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let result = client.get_block_hex(" 7 ").await.expect("block hex");
    assert_eq!(result, "beef");
}

#[test]
fn parse_plugins_errors_on_non_object() {
    let array = JToken::Array(JArray::from(vec![JToken::String("bad".into())]));
    let err = helpers::parse_plugins(&array).expect_err("should fail");
    assert_eq!(err.code(), -32603);
}

#[tokio::test]
async fn get_plugins_parses_listplugins_response_over_rpc() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                    "jsonrpc":"2.0",
                    "id":1,
                    "result":[
                        {"name":"RpcServer","version":"1.2.3","interfaces":[]}
                    ]
               }"#,
        )
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let plugins = client.get_plugins().await.expect("plugins parsed");
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "RpcServer");
    assert_eq!(plugins[0].version, "1.2.3");
    assert!(plugins[0].category.is_none());
}

#[tokio::test]
async fn send_raw_transaction_uses_base64_and_returns_hash() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendrawtransactionasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let base64_tx = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("base64 tx");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let result = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("case result");
    let expected_hash = result
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");

    let bytes = general_purpose::STANDARD
        .decode(base64_tx.as_bytes())
        .expect("decode tx");
    let mut reader = MemoryReader::new(&bytes);
    let tx = <Transaction as Serializable>::deserialize(&mut reader).expect("deserialize tx");

    let mut server = Server::new_async().await;
    let escaped = escape(&base64_tx);
    let body_re =
        format!(r#""method"\s*:\s*"sendrawtransaction".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
    let response_body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"hash":"{}"}}}}"#,
        expected_hash
    );
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual_hash = client
        .send_raw_transaction(&tx)
        .await
        .expect("send raw transaction");
    assert_eq!(
        actual_hash,
        UInt256::parse(&expected_hash).expect("parse expected hash")
    );
}

#[tokio::test]
async fn send_raw_transaction_propagates_error_response() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendrawtransactionasyncerror") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let base64_tx = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("base64 tx");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");

    let bytes = general_purpose::STANDARD
        .decode(base64_tx.as_bytes())
        .expect("decode tx");
    let mut reader = MemoryReader::new(&bytes);
    let tx = <Transaction as Serializable>::deserialize(&mut reader).expect("deserialize tx");

    let mut server = Server::new_async().await;
    let escaped = escape(&base64_tx);
    let body_re =
        format!(r#""method"\s*:\s*"sendrawtransaction".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response.to_string())
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let err = client
        .send_raw_transaction(&tx)
        .await
        .expect_err("send raw transaction error");
    assert_eq!(err.code(), -500);
    assert_eq!(err.message(), "InsufficientFunds");
}

#[tokio::test]
async fn send_async_returns_error_when_throw_is_false() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendrawtransactionasyncerror") else {
        return;
    };
    let request_obj = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let request = RpcRequest::from_json(request_obj).expect("request parse");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");

    let mut server = Server::new_async().await;
    // The client serializes requests with plain serde_json (not the C# escaper);
    // build the expected body the same way so Matcher::Exact matches.
    let request_body =
        serde_json::to_string(&JToken::Object(request.to_json())).expect("serialize request");
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Exact(request_body))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response.to_string())
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let response = client
        .send_async(request, false)
        .await
        .expect("no-throw response");
    assert!(response.result.is_none());
    let err = response.error.expect("error");
    assert_eq!(err.code, -500);
    assert_eq!(err.message, "InsufficientFunds");
    assert!(err.data.is_some());
}

#[tokio::test]
async fn rpc_client_with_basic_auth_sends_authorization_header() {
    if !localhost_binding_permitted() {
        return;
    }

    let user = "krain";
    let pass = "123456";
    let expected = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(format!("{user}:{pass}").as_bytes())
    );

    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .match_header("authorization", expected.as_str())
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getblockcount""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":7}"#)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url)
        .with_basic_auth(user, pass)
        .build()
        .unwrap();
    let count = client.get_block_count().await.expect("block count");
    assert_eq!(count, 7);
}

#[tokio::test]
async fn rpc_client_new_with_basic_auth_sends_authorization_header() {
    if !localhost_binding_permitted() {
        return;
    }

    let user = "krain";
    let pass = "123456";
    let expected = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(format!("{user}:{pass}").as_bytes())
    );

    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .match_header("authorization", expected.as_str())
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getblockcount""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":7}"#)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::new(url, Some(user.to_string()), Some(pass.to_string()), None)
        .expect("rpc client");
    let count = client.get_block_count().await.expect("block count");
    assert_eq!(count, 7);
}

#[tokio::test]
async fn submit_block_uses_base64_and_returns_hash() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut block = Block::new();
    block.rebuild_merkle_root();
    let expected_hash = block.hash().to_string();
    let base64_block = general_purpose::STANDARD.encode(block.to_array().expect("serialize block"));

    let mut server = Server::new_async().await;
    let escaped = escape(&base64_block);
    let body_re = format!(r#""method"\s*:\s*"submitblock".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
    let response_body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"hash":"{}"}}}}"#,
        expected_hash
    );
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual_hash = client.submit_block(&block).await.expect("submit block");
    assert_eq!(
        actual_hash,
        UInt256::parse(&expected_hash).expect("parse expected hash")
    );
}

#[tokio::test]
async fn invoke_script_uses_base64_and_parses_result() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("invokescriptasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let base64_script = params
            .get(0)
            .and_then(|value| value.as_string())
            .expect("base64 script");
        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let result = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("case result");
        let expected_script = result
            .get("script")
            .and_then(|value| value.as_string())
            .expect("script");
        let expected_gas = result
            .get("gasconsumed")
            .and_then(|value| value.as_string())
            .expect("gas consumed")
            .parse::<i64>()
            .expect("parse gas consumed");

        let bytes = general_purpose::STANDARD
            .decode(base64_script.as_bytes())
            .expect("decode script");

        let mut server = Server::new_async().await;
        let escaped = escape(&base64_script);
        let body_re =
            format!(r#""method"\s*:\s*"invokescript".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
        let response_body = JToken::Object(response.clone()).to_string();
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(body_re))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let actual = client.invoke_script(&bytes).await.expect("invoke script");
        assert_eq!(actual.script, expected_script);
        assert_eq!(actual.gas_consumed, expected_gas);
    }
}

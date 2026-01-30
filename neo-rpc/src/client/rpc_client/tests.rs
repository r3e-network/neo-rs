use super::*;
use crate::client::models::{
    RpcAccount, RpcContractState, RpcPlugin, RpcRawMemPool, RpcRequest, RpcTransferOut,
    RpcValidator,
};
use base64::{engine::general_purpose, Engine as _};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response, Server as HyperServer};
use mockito::Matcher;
use mockito::Server;
use neo_core::config::ProtocolSettings;
use neo_core::big_decimal::BigDecimal;
use neo_core::extensions::SerializableExtensions;
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::block::Block;
use neo_core::Transaction;
use neo_json::{JArray, JObject, JToken};
use neo_primitives::UInt256;
use num_bigint::BigInt;
use regex::escape;
use std::convert::Infallible;
use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;

fn localhost_binding_permitted() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
}

fn load_rpc_case(name: &str) -> JObject {
    load_rpc_cases(name)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("RpcTestCases.json missing case: {name}"))
}

fn load_rpc_cases(name: &str) -> Vec<JObject> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("neo_csharp");
    path.push("node");
    path.push("tests");
    path.push("Neo.Network.RPC.Tests");
    path.push("RpcTestCases.json");
    let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
    let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
    let cases = token
        .as_array()
        .expect("RpcTestCases.json should be an array");
    let mut matches = Vec::new();
    for entry in cases.children() {
        let token = entry.as_ref().expect("array entry");
        let obj = token.as_object().expect("case object");
        let case_name = obj
            .get("Name")
            .and_then(|value| value.as_string())
            .unwrap_or_default();
        if case_name.eq_ignore_ascii_case(name) {
            matches.push(obj.clone());
        }
    }
    matches
}

enum ContractStateRequest {
    Hash(String),
    Id(i32),
}

async fn start_slow_server(
    delay: Duration,
    body: &'static str,
) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    listener
        .set_nonblocking(true)
        .expect("configure test listener");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = HyperServer::from_tcp(listener)
        .expect("server from tcp")
        .serve(make_service_fn(move |_| {
            let body = body.to_string();
            async move {
                Ok::<_, Infallible>(service_fn(move |_| {
                    let body = body.clone();
                    async move {
                        tokio::time::sleep(delay).await;
                        Ok::<_, Infallible>(Response::new(Body::from(body.clone())))
                    }
                }))
            }
        }));

    let graceful = server.with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    tokio::spawn(graceful);

    (addr, shutdown_tx)
}

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
    let hooks = RpcClientHooks::new().with_observer({
        let calls = Arc::clone(&calls);
        let successful = Arc::clone(&successful);
        move |outcome: &RpcRequestOutcome| {
            calls.fetch_add(1, Ordering::SeqCst);
            successful.store(outcome.success, Ordering::SeqCst);
            assert_eq!(outcome.method, "getblockcount");
            assert!(outcome.elapsed > Duration::from_millis(0));
        }
    });

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).hooks(hooks).build().unwrap();
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

    let case = load_rpc_case("sendrawtransactionasync");
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

    let case = load_rpc_case("sendrawtransactionasyncerror");
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

    let case = load_rpc_case("sendrawtransactionasyncerror");
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
    let request_body = request.to_json().to_string();
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

    for case in load_rpc_cases("invokescriptasync") {
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

#[tokio::test]
async fn get_block_count_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getblockcountasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getblockcount".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_block_count().await.expect("block count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_hash_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getblockhashasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let index = params
        .get(0)
        .and_then(|value| value.as_number())
        .expect("block index") as u32;
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(r#""method"\s*:\s*"getblockhash".*"params"\s*:\s*\[\s*{index}\s*\]"#);
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
    let actual = client.get_block_hash(index).await.expect("block hash");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_header_count_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getblockheadercountasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getblockheadercount".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_block_header_count()
        .await
        .expect("block header count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_sys_fee_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getblocksysfeeasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let height = params
        .get(0)
        .and_then(|value| value.as_number())
        .expect("height") as u32;
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_token = response.get("result").expect("result token");
    let expected = if let Some(text) = expected_token.as_string() {
        BigInt::from_str(&text).expect("parse sysfee")
    } else if let Some(number) = expected_token.as_number() {
        BigInt::from(number as i64)
    } else {
        panic!("invalid sysfee token");
    };

    let mut server = Server::new_async().await;
    let body_re = format!(r#""method"\s*:\s*"getblocksysfee".*"params"\s*:\s*\[\s*{height}\s*\]"#);
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
    let actual = client
        .get_block_sys_fee(height)
        .await
        .expect("block sys fee");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_header_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getblockheaderhexasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*{index}\s*\]"#),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#),
            )
        } else {
            panic!("invalid getblockheader param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let mut server = Server::new_async().await;
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
        let actual = client
            .get_block_header_hex(&hash_or_index)
            .await
            .expect("block header hex");
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn get_block_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getblockhexasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*{index}\s*\]"#),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#),
            )
        } else {
            panic!("invalid getblock param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let mut server = Server::new_async().await;
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
        let actual = client
            .get_block_hex(&hash_or_index)
            .await
            .expect("block hex");
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn get_raw_mempool_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getrawmempoolasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result")
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_string())
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getrawmempool".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_raw_mempool().await.expect("raw mempool");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_raw_mempool_both_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getrawmempoolbothasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_result = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");
    let expected = RpcRawMemPool::from_json(expected_result).expect("parse expected mempool");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getrawmempool".*"params"\s*:\s*\[\s*true\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_raw_mempool_both()
        .await
        .expect("raw mempool both");
    assert_eq!(actual.height, expected.height);
    assert_eq!(actual.verified, expected.verified);
    assert_eq!(actual.unverified, expected.unverified);
}

#[tokio::test]
async fn get_raw_transaction_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getrawtransactionhexasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let escaped = escape(&hash);
    let body_re =
        format!(r#""method"\s*:\s*"getrawtransaction".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
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
    let actual = client
        .get_raw_transaction_hex(&hash)
        .await
        .expect("raw transaction hex");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_nep17_balances_parses_rpc_payload() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let address = neo_primitives::UInt160::zero().to_address();
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"balance":[{{"assethash":"0x0000000000000000000000000000000000000000","amount":"5","lastupdatedblock":7}}],"address":"{address}"}}}}"#
    );

    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let balances = client
        .get_nep17_balances(&address)
        .await
        .expect("parse balances");
    assert_eq!(balances.user_script_hash, neo_primitives::UInt160::zero());
    assert_eq!(balances.balances.len(), 1);
    assert_eq!(balances.balances[0].last_updated_block, 7);
}

#[tokio::test]
async fn get_nep17_transfers_parses_rpc_payload() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let address = neo_primitives::UInt160::zero().to_address();
    let tx_hash = neo_primitives::UInt256::zero();
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"address":"{address}","sent":[],"received":[{{"assethash":"0x0000000000000000000000000000000000000000","transferaddress":"{address}","amount":"1","blockindex":7,"transfernotifyindex":0,"timestamp":0,"txhash":"{tx_hash}"}}]}}}}"#
    );

    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let transfers = client
        .get_nep17_transfers(&address, None, None)
        .await
        .expect("parse transfers");
    assert_eq!(transfers.user_script_hash, neo_primitives::UInt160::zero());
    assert!(transfers.sent.is_empty());
    assert_eq!(transfers.received.len(), 1);
    assert_eq!(transfers.received[0].amount.to_string(), "1");
}

#[tokio::test]
async fn get_storage_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getstorageasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let hash_or_id = params.get(0).expect("hash or id");
        let key = params
            .get(1)
            .and_then(|value| value.as_string())
            .expect("key");
        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let (body_re, hash_or_id) = if let Some(hash) = hash_or_id.as_string() {
            let escaped_hash = escape(&hash);
            let escaped_key = escape(&key);
            let body_re = format!(
                r#""method"\s*:\s*"getstorage".*"params"\s*:\s*\[\s*"{escaped_hash}"\s*,\s*"{escaped_key}"\s*\]"#
            );
            (body_re, hash.to_string())
        } else if let Some(id) = hash_or_id.as_number() {
            let id = id as i32;
            let id_string = id.to_string();
            let escaped_key = escape(&key);
            let body_re = format!(
                r#""method"\s*:\s*"getstorage".*"params"\s*:\s*\[\s*{id}\s*,\s*"{escaped_key}"\s*\]"#
            );
            (body_re, id_string)
        } else {
            panic!("invalid getstorage hash");
        };

        let mut server = Server::new_async().await;
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
        let actual = client
            .get_storage(&hash_or_id, &key)
            .await
            .expect("storage value");
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn get_connection_count_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getconnectioncountasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getconnectioncount".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_connection_count()
        .await
        .expect("connection count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_committee_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getcommitteeasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result")
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_string())
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getcommittee".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_committee().await.expect("committee");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_next_block_validators_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnextblockvalidatorsasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_array = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result");
    let expected = expected_array
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcValidator::from_json(obj).expect("validator"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnextblockvalidators".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_next_block_validators()
        .await
        .expect("validators");
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual[0].public_key, expected[0].public_key);
    assert_eq!(actual[0].votes, expected[0].votes);
}

#[tokio::test]
async fn get_transaction_height_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("gettransactionheightasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let mut server = Server::new_async().await;
    let escaped = escape(&hash);
    let body_re =
        format!(r#""method"\s*:\s*"gettransactionheight".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
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
    let actual = client
        .get_transaction_height(&hash)
        .await
        .expect("transaction height");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_native_contracts_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnativecontractsasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_array = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result");
    let expected = expected_array
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcContractState::from_json(obj).expect("contract state"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnativecontracts".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_native_contracts()
        .await
        .expect("native contracts");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.contract_state, right.contract_state);
    }
}

#[tokio::test]
async fn list_plugins_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("listpluginsasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_array = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result");
    let expected = expected_array
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcPlugin::from_json(obj).expect("plugin"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"listplugins".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_plugins().await.expect("plugins");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.name, right.name);
        assert_eq!(left.version, right.version);
        assert_eq!(left.interfaces, right.interfaces);
        assert_eq!(left.category, right.category);
    }
}

#[tokio::test]
async fn close_wallet_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("closewalletasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .map(|value| value.as_boolean())
        .unwrap_or(false);

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"closewallet".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.close_wallet().await.expect("closewallet");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn open_wallet_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("openwalletasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let path = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("path");
    let password = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("password");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .map(|value| value.as_boolean())
        .unwrap_or(false);

    let mut server = Server::new_async().await;
    let escaped_path = escape(
        serde_json::to_string(&path)
            .expect("json path")
            .trim_matches('"'),
    );
    let escaped_password = escape(&password);
    let body_re = format!(
        r#""method"\s*:\s*"openwallet".*"params"\s*:\s*\[\s*"{escaped_path}"\s*,\s*"{escaped_password}"\s*\]"#
    );
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
    let actual = client
        .open_wallet(&path, &password)
        .await
        .expect("openwallet");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_new_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnewaddressasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnewaddress".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_new_address().await.expect("new address");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn dump_priv_key_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("dumpprivkeyasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let escaped = escape(&address);
    let body_re = format!(r#""method"\s*:\s*"dumpprivkey".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
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
    let actual = client.dump_priv_key(&address).await.expect("dumpprivkey");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn list_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("listaddressasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_array = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result");
    let expected = expected_array
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcAccount::from_json(obj).expect("account"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"listaddress".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.list_address().await.expect("listaddress");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.address, right.address);
        assert_eq!(left.has_key, right.has_key);
        assert_eq!(left.label, right.label);
        assert_eq!(left.watch_only, right.watch_only);
    }
}

#[tokio::test]
async fn get_wallet_balance_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getwalletbalanceasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let asset_id = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("asset");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let balance = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("balance"))
        .and_then(|value| value.as_string())
        .expect("balance");
    let expected_value = BigInt::from_str(&balance).expect("parse balance");
    let expected = BigDecimal::new(expected_value, 8);

    let mut server = Server::new_async().await;
    let escaped_asset = escape(&asset_id);
    let wallet_body_re =
        format!(r#""method"\s*:\s*"getwalletbalance".*"params"\s*:\s*\[\s*"{escaped_asset}"\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m_wallet = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(wallet_body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let invoke_body = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"8"}]}}"#;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_wallet_balance(&asset_id)
        .await
        .expect("wallet balance");
    assert_eq!(actual.value(), expected.value());
    assert_eq!(actual.decimals(), expected.decimals());
}

#[tokio::test]
async fn get_wallet_unclaimed_gas_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getwalletunclaimedgasasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let amount = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");
    let expected_value = BigInt::from_str(&amount).expect("parse amount");
    let expected = BigDecimal::new(expected_value, 8);

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getwalletunclaimedgas".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_wallet_unclaimed_gas()
        .await
        .expect("wallet unclaimed gas");
    assert_eq!(actual.value(), expected.value());
    assert_eq!(actual.decimals(), expected.decimals());
}

#[tokio::test]
async fn send_from_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("sendfromasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let asset_id = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("asset");
    let from_addr = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("from");
    let to_addr = params
        .get(2)
        .and_then(|value| value.as_string())
        .expect("to");
    let amount = params
        .get(3)
        .and_then(|value| value.as_string())
        .expect("amount");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_hash = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("hash"))
        .and_then(|value| value.as_string())
        .expect("hash");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"sendfrom".*"params"\s*:\s*\[\s*"{asset}"\s*,\s*"{from}"\s*,\s*"{to}"\s*,\s*"{amount}"\s*\]"#,
        asset = escape(&asset_id),
        from = escape(&from_addr),
        to = escape(&to_addr),
        amount = escape(&amount),
    );
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
    let actual = client
        .send_from(&asset_id, &from_addr, &to_addr, &amount)
        .await
        .expect("sendfrom");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}

#[tokio::test]
async fn get_best_block_hash_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getbestblockhashasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getbestblockhash".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_best_block_hash().await.expect("best block hash");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_verbose_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getblockasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(
                    r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*{index}\s*,\s*true\s*\]"#
                ),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(
                    r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*"{escaped}"\s*,\s*true\s*\]"#,
                    escaped = escaped
                ),
            )
        } else {
            panic!("invalid getblock param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let mut server = Server::new_async().await;
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
        let actual = client
            .get_block_verbose(&hash_or_index)
            .await
            .expect("block");
        let settings = ProtocolSettings::default_settings();
        assert_eq!(actual.to_json(&settings), expected.clone());
    }
}

#[tokio::test]
async fn get_block_header_verbose_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getblockheaderasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(
                    r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*{index}\s*,\s*true\s*\]"#
                ),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(
                    r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*"{escaped}"\s*,\s*true\s*\]"#,
                    escaped = escaped
                ),
            )
        } else {
            panic!("invalid getblockheader param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let mut server = Server::new_async().await;
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
        let actual = client
            .get_block_header_verbose(&hash_or_index)
            .await
            .expect("block header");
        let settings = ProtocolSettings::default_settings();
        assert_eq!(actual.to_json(&settings), expected.clone());
    }
}

#[tokio::test]
async fn get_transaction_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getrawtransactionasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getrawtransaction".*"params"\s*:\s*\[\s*"{hash}"\s*,\s*true\s*\]"#,
        hash = escape(&hash)
    );
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
    let actual = client.get_transaction(&hash).await.expect("transaction");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn invoke_function_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("invokefunctionasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let contract = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("contract");
    let operation = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("operation");
    let stack_params = params
        .get(2)
        .and_then(|value| value.as_array())
        .expect("stack params");
    let args = stack_params
        .children()
        .iter()
        .filter_map(|item| item.clone())
        .collect::<Vec<_>>();

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"invokefunction".*"params"\s*:\s*\[\s*"{contract}".*"{operation}".*\]"#,
        contract = escape(&contract),
        operation = escape(&operation)
    );
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
    let actual = client
        .invoke_function(&contract, &operation, &args)
        .await
        .expect("invoke function");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_contract_state_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    for case in load_rpc_cases("getcontractstateasync") {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let contract = params.get(0).expect("contract");

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let (body_re, request) = if let Some(name) = contract.as_string() {
            let body_re = format!(
                r#""method"\s*:\s*"getcontractstate".*"params"\s*:\s*\[\s*"{contract}"\s*\]"#,
                contract = escape(&name)
            );
            (body_re, ContractStateRequest::Hash(name.to_string()))
        } else if let Some(id) = contract.as_number() {
            let id = id as i32;
            let body_re =
                format!(r#""method"\s*:\s*"getcontractstate".*"params"\s*:\s*\[\s*{id}\s*\]"#);
            (body_re, ContractStateRequest::Id(id))
        } else {
            panic!("invalid getcontractstate param");
        };

        let mut server = Server::new_async().await;
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
        let actual = match request {
            ContractStateRequest::Hash(hash) => client.get_contract_state(&hash).await,
            ContractStateRequest::Id(id) => client.get_contract_state_by_id(id).await,
        }
        .expect("contract state");
        assert_eq!(actual.to_json().expect("to json"), expected.clone());
    }
}

#[tokio::test]
async fn get_peers_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getpeersasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getpeers".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_peers().await.expect("peers");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_version_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getversionasync");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getversion".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_version().await.expect("version");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_application_log_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getapplicationlogasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getapplicationlog".*"params"\s*:\s*\[\s*"{hash}"\s*\]"#,
        hash = escape(&hash)
    );
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
    let actual = client
        .get_application_log(&hash)
        .await
        .expect("application log");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_application_log_with_trigger_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getapplicationlogasync_triggertype");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");
    let trigger = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("trigger");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getapplicationlog".*"params"\s*:\s*\[\s*"{hash}"\s*,\s*"{trigger}"\s*\]"#,
        hash = escape(&hash),
        trigger = escape(&trigger)
    );
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
    let actual = client
        .get_application_log_with_trigger(&hash, &trigger)
        .await
        .expect("application log");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn validate_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("validateaddressasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"validateaddress".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
    );
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
    let actual = client
        .validate_address(&address)
        .await
        .expect("validate address");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn import_priv_key_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("importprivkeyasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let wif = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("wif");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"importprivkey".*"params"\s*:\s*\[\s*"{wif}"\s*\]"#,
        wif = escape(&wif)
    );
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
    let actual = client.import_priv_key(&wif).await.expect("import");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_unclaimed_gas_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getunclaimedgasasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getunclaimedgas".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
    );
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
    let actual = client
        .get_unclaimed_gas(&address)
        .await
        .expect("unclaimed gas");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_nep17_transfers_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnep17transfersasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");
    let start_time = params
        .get(1)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);
    let end_time = params
        .get(2)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17transfers".*"params"\s*:\s*\[\s*"{address}""#,
        address = escape(&address)
    );
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
    let actual = client
        .get_nep17_transfers(&address, start_time, end_time)
        .await
        .expect("nep17 transfers");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn get_nep17_transfers_accepts_null_transfer_address() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnep17transfersasync_with_null_transferaddress");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");
    let start_time = params
        .get(1)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);
    let end_time = params
        .get(2)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17transfers".*"params"\s*:\s*\[\s*"{address}""#,
        address = escape(&address)
    );
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
    let actual = client
        .get_nep17_transfers(&address, start_time, end_time)
        .await
        .expect("nep17 transfers");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn get_nep17_balances_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("getnep17balancesasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17balances".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
    );
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
    let actual = client
        .get_nep17_balances(&address)
        .await
        .expect("nep17 balances");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn send_to_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("sendtoaddressasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let asset_id = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("asset");
    let address = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("address");
    let amount = params
        .get(2)
        .and_then(|value| value.as_string())
        .expect("amount");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_hash = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("hash"))
        .and_then(|value| value.as_string())
        .expect("hash");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"sendtoaddress".*"params"\s*:\s*\[\s*"{asset}"\s*,\s*"{address}"\s*,\s*"{amount}"\s*\]"#,
        asset = escape(&asset_id),
        address = escape(&address),
        amount = escape(&amount),
    );
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
    let actual = client
        .send_to_address(&asset_id, &address, &amount)
        .await
        .expect("sendtoaddress");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}

#[tokio::test]
async fn send_many_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let case = load_rpc_case("sendmanyasync");
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let from_addr = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("from");
    let outputs = params
        .get(1)
        .and_then(|value| value.as_array())
        .expect("outputs");
    let settings = ProtocolSettings::default_settings();
    let parsed_outputs = outputs
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcTransferOut::from_json(obj, &settings).expect("output"))
        .collect::<Vec<_>>();
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_hash = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("hash"))
        .and_then(|value| value.as_string())
        .expect("hash");

    let mut server = Server::new_async().await;
    let escaped_from = escape(&from_addr);
    let first_asset = outputs
        .get(0)
        .and_then(|token| token.as_object())
        .and_then(|obj| obj.get("asset"))
        .and_then(|value| value.as_string())
        .expect("asset");
    let second_asset = outputs
        .get(1)
        .and_then(|token| token.as_object())
        .and_then(|obj| obj.get("asset"))
        .and_then(|value| value.as_string())
        .expect("asset");
    let body_re = format!(
        r#"(?s)"method"\s*:\s*"sendmany".*"params"\s*:\s*\[\s*"{escaped_from}".*"{first_asset}".*"{second_asset}".*\]"#,
        escaped_from = escaped_from,
        first_asset = escape(&first_asset),
        second_asset = escape(&second_asset),
    );
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
    let actual = client
        .send_many(&from_addr, &parsed_outputs)
        .await
        .expect("sendmany");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}

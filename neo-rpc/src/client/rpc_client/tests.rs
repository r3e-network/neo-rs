use super::*;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response, Server as HyperServer};
use mockito::Server;
use neo_json::{JArray, JObject, JToken};
use std::convert::Infallible;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;

fn localhost_binding_permitted() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
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

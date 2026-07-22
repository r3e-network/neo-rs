use super::*;
use crate::client::models::{RpcAccount, RpcPlugin, RpcRequest, RpcTransferOut, RpcValidator};
use crate::types::{RpcContractState, RpcRawMemPool};
use base64::{Engine as _, engine::general_purpose};
// `hyper` was removed from these tests in the
// `2026-06-13-comprehensive-refactoring` change (Phase C1) when the RPC server
// moved to jsonrpsee. The few integration tests that used to spin up an
// in-process hyper server now use `mockito` exclusively.
use mockito::{Matcher, Mock, Server, ServerGuard};
use neo_config::ProtocolSettings;
use neo_io::SerializableExtensions;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::Transaction;
use neo_payloads::block::Block;
use neo_primitives::BigDecimal;
use neo_primitives::UInt256;
use neo_serialization::json::{JArray, JObject, JToken};
use num_bigint::BigInt;
use regex::escape;
use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::oneshot;

fn localhost_binding_permitted() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
}

fn load_rpc_case(name: &str) -> Option<JObject> {
    let cases = load_rpc_cases(name)?;
    let result = cases.into_iter().next();
    if result.is_none() {
        eprintln!("SKIP: RpcTestCases.json missing case: {name}");
    }
    result
}

fn load_rpc_cases(name: &str) -> Option<Vec<JObject>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("neo_csharp");
    path.push("node");
    path.push("tests");
    path.push("Neo.Network.RPC.Tests");
    path.push("RpcTestCases.json");
    if !path.exists() {
        eprintln!(
            "SKIP: neo_csharp submodule not initialized ({})",
            path.display()
        );
        return None;
    }
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
    Some(matches)
}

#[test]
fn get_rpc_name_strips_csharp_suffixes() {
    for (method, expected) in [
        ("GetBlockAsync", "getblock"),
        ("GetRawTransactionHexAsync", "getrawtransaction"),
        ("GetRawMemPoolBothAsync", "getrawmempool"),
        ("GetVersion", "getversion"),
    ] {
        assert_eq!(RpcClient::get_rpc_name(method), expected);
    }
}

enum ContractStateRequest {
    Hash(String),
    Id(i32),
}

struct RpcFixture {
    client: RpcClient,
    response: JObject,
    _mock: Mock,
    _server: ServerGuard,
}

async fn mock_no_param_fixture(case_name: &str, method: &str) -> Option<RpcFixture> {
    if !localhost_binding_permitted() {
        return None;
    }

    let case = load_rpc_case(case_name)?;
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response")
        .clone();

    let mut server = Server::new_async().await;
    let method = escape(method);
    let body_re = format!(r#""method"\s*:\s*"{method}".*"params"\s*:\s*\[\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let mock = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    Some(RpcFixture {
        client,
        response,
        _mock: mock,
        _server: server,
    })
}

async fn start_slow_server(
    delay: Duration,
    body: &'static str,
) -> (SocketAddr, oneshot::Sender<()>) {
    // Replaces the previous hyper-based test server (removed in the
    // `2026-06-13-comprehensive-refactoring` change, Phase C1). The
    // minimal replacement is a raw TCP listener that accepts a
    // connection, sleeps for `delay`, writes `body` as an HTTP/1.1
    // response, and closes — sufficient for the client-side timeout
    // / retry tests that previously relied on this helper.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    listener
        .set_nonblocking(true)
        .expect("configure test listener");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let mut shutdown_rx = shutdown_rx;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let mut stream = tokio::net::TcpStream::from_std(stream)
                                .expect("convert to tokio TcpStream");
                            tokio::time::sleep(delay).await;
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            use tokio::io::AsyncWriteExt;
                            let _ = stream.write_all(response.as_bytes()).await;
                            let _ = stream.shutdown().await;
                        }
                        Err(_) => {
                            // spurious wakeup, continue
                        }
                    }
                }
            }
        }
    });

    (addr, shutdown_tx)
}

#[path = "rpc_client/blockchain_basic.rs"]
mod blockchain_basic;
#[path = "rpc_client/blockchain_verbose.rs"]
mod blockchain_verbose;
#[path = "rpc_client/node.rs"]
mod node;
#[path = "rpc_client/transport.rs"]
mod transport;
#[path = "rpc_client/wallet.rs"]
mod wallet;
#[path = "rpc_client/wallet_and_nep17.rs"]
mod wallet_and_nep17;

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.contains(&format!(r#""method":"{expected_method}""#))
                || text.contains(&format!(r#""method": "{expected_method}""#)),
            "unexpected request: {text}"
        );
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result,
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    url
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_defaults_to_verified_hashes() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let result = (handler.callback())(&server, &[]).expect("getrawmempool");
    let array = result.as_array().expect("array result");
    assert!(array.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_verbose_roundtrips_into_client_model() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let params = [Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("getrawmempool verbose");
    let parsed = RpcRawMemPool::from_json(&parse_object(&result)).expect("parse mempool");
    assert!(parsed.verified.is_empty());
    assert!(parsed.unverified.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_uses_remote_ledger_rpc_when_configured() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server
        .set_remote_ledger_rpc(serve_rpc_once(
            "getrawmempool",
            serde_json::json!({
                "height": 321,
                "verified": ["0xremote"],
                "unverified": []
            }),
        ))
        .expect("configure remote ledger RPC");
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let callback = handler.callback();
    let server = Arc::new(server);
    let result = tokio::task::spawn_blocking(move || {
        (callback)(&server, &[Value::Bool(true)]).expect("remote getrawmempool")
    })
    .await
    .expect("blocking handler task");

    assert_eq!(result["height"], 321);
    assert_eq!(result["verified"], serde_json::json!(["0xremote"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_mem_pool_mixed_verified_and_unverified() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawmempool");

    let keypair_a = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair a");
    let keypair_b = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair b");
    let keypair_c = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair c");

    let account_a = keypair_a.script_hash();
    let account_b = keypair_b.script_hash();
    let account_c = keypair_c.script_hash();

    let mut store = system.store_cache();
    let funded = BigInt::from(50_0000_0000i64);
    mint_gas(&mut store, &settings, account_a, funded.clone());
    mint_gas(&mut store, &settings, account_b, funded.clone());
    mint_gas(&mut store, &settings, account_c, funded);
    store.try_commit().expect("commit test store");

    let tx1 = build_signed_transaction(&settings, &keypair_a, 1);
    let tx2 = build_signed_transaction(&settings, &keypair_b, 2);
    let tx3 = build_signed_transaction(&settings, &keypair_c, 3);

    let pool_arc = system.mempool();
    {
        let pool = &pool_arc;
        assert_eq!(
            pool.try_add(tx1.clone(), store.data_cache()),
            VerifyResult::Succeed
        );
        assert_eq!(
            pool.try_add(tx2.clone(), store.data_cache()),
            VerifyResult::Succeed
        );

        let block = Block::new();
        let removed = pool.update_pool_for_block_persisted(&block.transactions);
        assert!(removed.is_empty());
        assert_eq!(pool.verified_count(), 0);
        assert_eq!(pool.unverified_count(), 2);

        assert_eq!(
            pool.try_add(tx3.clone(), store.data_cache()),
            VerifyResult::Succeed
        );
        assert_eq!(pool.verified_count(), 1);
        assert_eq!(pool.unverified_count(), 2);
    }

    let params = [Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("getrawmempool verbose");
    let parsed = RpcRawMemPool::from_json(&parse_object(&result)).expect("parse mempool");

    let verified_hashes: HashSet<String> = parsed
        .verified
        .iter()
        .map(|hash| hash.to_string())
        .collect();
    let unverified_hashes: HashSet<String> = parsed
        .unverified
        .iter()
        .map(|hash| hash.to_string())
        .collect();

    assert!(verified_hashes.contains(&tx3.hash().to_string()));
    assert!(unverified_hashes.contains(&tx1.hash().to_string()));
    assert!(unverified_hashes.contains(&tx2.hash().to_string()));
    assert_eq!(verified_hashes.len(), 1);
    assert_eq!(unverified_hashes.len(), 2);
}

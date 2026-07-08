use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_payloads::Header;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

#[test]
fn block_handlers_use_shared_ledger_query_boundary() {
    let handlers = include_str!("../../../server/rpc_server_blockchain/blocks.rs");

    assert!(
        handlers.contains("ledger_queries::current_hash"),
        "getbestblockhash should route current-hash reads through shared ledger queries"
    );
    assert!(
        handlers.contains("ledger_queries::block_count"),
        "getblockcount should route block-count reads through shared ledger queries"
    );
    assert!(
        handlers.contains("ledger_queries::current_index_and_next_hash"),
        "verbose block/header responses should route tip + next-hash reads through shared ledger queries"
    );
    assert!(
        handlers.contains("ledger_queries::get_full_block"),
        "block payload reconstruction should stay on the shared ledger-query boundary"
    );
    assert!(
        !handlers.contains("StorageLedgerProviderFactory"),
        "block RPC handlers should not construct raw storage ledger providers directly"
    );
    assert!(
        !handlers.contains("LedgerContract::new()"),
        "block RPC handlers should not construct native LedgerContract directly"
    );
}

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
async fn get_best_block_hash_reflects_current_state() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getbestblockhash");

    let mut store = system.store_cache();
    let hash = UInt256::zero();
    let index = 100u32;
    // C# `HashIndexState` interoperable stack item, matching the reader.
    let current_bytes = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, index)
        .expect("serialize HashIndexState pointer");
    let key = StorageKey::new(LedgerContract::ID, vec![0x0c]);
    store.add(key, StorageItem::from_bytes(current_bytes));
    store.commit();

    let result = (handler.callback())(&server, &[]).expect("get best block hash");
    assert_eq!(result.as_str().expect("hash"), hash.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_count_defaults_to_one() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockcount");

    let result = (handler.callback())(&server, &[]).expect("get block count");
    assert_eq!(result.as_u64().unwrap_or_default(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_count_uses_remote_ledger_rpc_when_configured() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server
        .set_remote_ledger_rpc(serve_rpc_once("getblockcount", Value::from(123u64)))
        .expect("configure remote ledger RPC");
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockcount");

    let callback = handler.callback();
    let server = Arc::new(server);
    let result = tokio::task::spawn_blocking(move || {
        (callback)(&server, &[]).expect("get remote block count")
    })
    .await
    .expect("blocking handler task");

    assert_eq!(result.as_u64().unwrap_or_default(), 123);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_count_defaults_to_one() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheadercount");

    let result = (handler.callback())(&server, &[]).expect("get block header count");
    assert_eq!(result.as_u64().unwrap_or_default(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_sums_transaction_fees() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let mut tx1 = make_transaction(1);
    tx1.set_system_fee(100_000_000);
    let mut tx2 = make_transaction(2);
    tx2.set_system_fee(200_000_000);
    let block = make_ledger_block(&system.store_cache(), 100, vec![tx1, tx2]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(100u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block sys fee");
    assert_eq!(result.as_str().expect("sys fee"), "300000000");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_rejects_invalid_param() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let params = [Value::String("not-a-number".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid params");
    assert_eq!(err.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_sys_fee_reports_unknown_height() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblocksysfee");

    let params = [Value::Number(1u32.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown height");
    assert_eq!(err.code(), RpcError::unknown_height().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_hash_reports_hash_for_height() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockhash");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(1)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block hash");
    assert_eq!(result.as_str().expect("hash"), block.hash().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_roundtrips_by_hash_and_index() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(1)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let hash_params = [Value::String(block.hash().to_string())];
    let result = (handler.callback())(&server, &hash_params).expect("get block by hash");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&decoded_clone), block.hash());

    let index_params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &index_params).expect("get block by index");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&decoded_clone), block.hash());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_genesis_roundtrips_and_reports_empty_txs() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    // The reth-style Node does not synthesise a genesis block at
    // construction (no genesis builder exists in-tree yet); persist a
    // synthetic empty block 0 with the same ledger records instead.
    let mut store = system.store_cache();
    let genesis = make_ledger_block(&store, 0, Vec::new());
    store_block(&mut store, &genesis);
    let genesis_hash = genesis.hash();

    let params = [Value::Number(0u32.into())];
    let result = (handler.callback())(&server, &params).expect("get genesis block");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&decoded_clone), genesis_hash);
    assert!(decoded.transactions.is_empty());

    let params = [Value::Number(0u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get genesis verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        genesis_hash.to_string()
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert!(txs.is_empty());
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_no_transactions_reports_empty_txs() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(&system.store_cache(), 1, Vec::new());
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into())];
    let result = (handler.callback())(&server, &params).expect("get block");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Block as Serializable>::deserialize(&mut reader).expect("deserialize block");
    let decoded_clone = decoded.clone();
    assert_eq!(Block::hash(&decoded_clone), block.hash());
    assert!(decoded.transactions.is_empty());

    let params = [Value::Number(1u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert!(txs.is_empty());
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_verbose_reports_confirmations() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(2)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(1u32.into()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    let txs = obj.get("tx").and_then(Value::as_array).expect("tx array");
    assert_eq!(txs.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_rejects_null_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_roundtrips() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheader");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(3)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::String(block.hash().to_string())];
    let result = (handler.callback())(&server, &params).expect("get block header");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Header as Serializable>::deserialize(&mut reader).expect("header");
    assert_eq!(decoded.index(), 1);

    let params = [Value::String(block.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get block header verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap(),
        block.hash().to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_header_rejects_null_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockheader");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

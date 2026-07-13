use super::RpcLedgerBlockSource;
use base64::Engine as _;
use neo_io::{BinaryWriter, Serializable};
use neo_network::BlockSource;
use neo_payloads::{Signer, Witness};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

fn empty_block(index: u32) -> neo_payloads::Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    neo_payloads::Block::from_parts(header, Vec::new())
}

fn test_transaction(nonce: u32) -> neo_payloads::Transaction {
    let mut tx = neo_payloads::Transaction::new();
    tx.set_version(0);
    tx.set_nonce(nonce);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn serialized_hex<T: Serializable>(payload: &T) -> String {
    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).expect("serialize payload");
    hex::encode(writer.into_bytes())
}

fn serialized_base64<T: Serializable>(payload: &T) -> String {
    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).expect("serialize payload");
    base64::engine::general_purpose::STANDARD.encode(writer.into_bytes())
}

fn test_mempool() -> Arc<neo_mempool::MemoryPool> {
    let settings = neo_config::ProtocolSettings::default();
    Arc::new(neo_mempool::MemoryPool::new_with_native_contract_provider(
        &settings,
        Arc::new(neo_native_contracts::StandardNativeProvider::new()),
    ))
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
        let body = json!({
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

#[test]
fn rpc_ledger_block_source_decodes_raw_block_by_index() {
    let block = empty_block(7);
    let url = serve_rpc_once("getblock", json!(serialized_hex(&block)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source.block_by_index(7).expect("remote block");

    assert_eq!(fetched.index(), 7);
    assert_eq!(fetched.hash(), block.hash());
}

#[test]
fn rpc_ledger_block_source_decodes_base64_raw_block_by_index() {
    let block = empty_block(17);
    let url = serve_rpc_once("getblock", json!(serialized_base64(&block)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source.block_by_index(17).expect("remote block");

    assert_eq!(fetched.index(), 17);
    assert_eq!(fetched.hash(), block.hash());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_ledger_block_source_can_be_called_from_async_runtime() {
    let block = empty_block(11);
    let url = serve_rpc_once("getblock", json!(serialized_hex(&block)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source.block_by_index(11).expect("remote block");

    assert_eq!(fetched.index(), 11);
    assert_eq!(fetched.hash(), block.hash());
}

#[test]
fn rpc_ledger_block_source_decodes_raw_header_by_index() {
    let block = empty_block(9);
    let url = serve_rpc_once("getblockheader", json!(serialized_hex(&block.header)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source.header_by_index(9).expect("remote header");

    assert_eq!(fetched.index(), 9);
    assert_eq!(fetched.hash(), block.header.hash());
}

#[test]
fn rpc_ledger_block_source_decodes_base64_raw_header_by_index() {
    let block = empty_block(19);
    let url = serve_rpc_once("getblockheader", json!(serialized_base64(&block.header)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source.header_by_index(19).expect("remote header");

    assert_eq!(fetched.index(), 19);
    assert_eq!(fetched.hash(), block.header.hash());
}

#[test]
fn rpc_ledger_block_source_decodes_raw_transaction_by_hash() {
    let tx = test_transaction(91);
    let tx_hash = tx.try_hash().expect("tx hash");
    let url = serve_rpc_once("getrawtransaction", json!(serialized_base64(&tx)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    let fetched = source
        .transaction_by_hash(&tx_hash)
        .expect("remote transaction");

    assert_eq!(fetched.try_hash().expect("fetched tx hash"), tx_hash);
}

#[test]
fn rpc_ledger_block_source_contains_transaction_uses_remote_rpc() {
    let tx = test_transaction(117);
    let tx_hash = tx.try_hash().expect("tx hash");
    let url = serve_rpc_once("getrawtransaction", json!(serialized_base64(&tx)));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    assert!(
        source.contains_transaction(&tx_hash),
        "remote-ledger mode should treat upstream transactions as known"
    );
}

#[test]
fn rpc_ledger_block_source_mempool_hashes_use_remote_rpc() {
    let tx_hash =
        UInt256::parse("0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
            .expect("tx hash");
    let url = serve_rpc_once("getrawmempool", json!([tx_hash.to_string()]));
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    assert_eq!(source.mempool_transaction_hashes(), vec![tx_hash]);
}

#[test]
fn rpc_ledger_block_source_mempool_hashes_accept_verbose_remote_shape() {
    let tx_hash =
        UInt256::parse("0x202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f")
            .expect("tx hash");
    let url = serve_rpc_once(
        "getrawmempool",
        json!({
            "height": 42,
            "verified": [tx_hash.to_string()],
            "unverified": [],
        }),
    );
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    assert_eq!(source.mempool_transaction_hashes(), vec![tx_hash]);
}

#[test]
fn rpc_ledger_block_source_returns_none_on_rpc_error() {
    let url = serve_rpc_once("getblock", Value::Null);
    let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

    assert!(source.block_by_index(1).is_none());
}

//! Integration tests for `getrawtransaction` VM-state response compatibility.
#![cfg(feature = "server")]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::transaction::Transaction;
use neo_primitives::{UInt160, WitnessScope};
use neo_rpc::server::{RpcHandler, RpcServer, RpcServerBlockchain, RpcServerConfig};
use neo_storage::persistence::providers::RuntimeStore;
use neo_test_fixtures::{TestTransactionBuilder, try_make_ledger_block, try_store_block};
use serde_json::Value;
use std::sync::Arc;

fn node_to_context(node: &neo_system::Node) -> neo_rpc::server::NodeContext {
    neo_rpc::server::NodeContext::from_parts(
        node.settings(),
        Arc::new(RuntimeStore::Memory(node.storage().as_ref().clone())),
        node.blockchain(),
        node.network(),
        node.mempool(),
        node.header_cache(),
        neo_rpc::server::RpcServices::default(),
        node.native_contract_provider(),
    )
}

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name == name)
        .expect("handler")
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_verbose_omits_vmstate() {
    let node = neo_system::Node::new(
        std::sync::Arc::new(neo_config::ProtocolSettings::default()),
        None,
        None,
    )
    .expect("system to start");
    let system: std::sync::Arc<neo_rpc::server::NodeContext> =
        std::sync::Arc::new(node_to_context(&node));
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let tx = TestTransactionBuilder::new()
        .nonce(7)
        .signer(
            UInt160::from_bytes(&[0x11; 20]).expect("account"),
            WitnessScope::CALLED_BY_ENTRY,
        )
        .build();
    let block = try_make_ledger_block(&system.store_cache(), 1, vec![tx.clone()])
        .expect("make ledger block fixture");
    let block_hash = block.hash();
    let mut store = system.store_cache();
    try_store_block(&mut store, &block).expect("store ledger block fixture");

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("blockhash")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        block_hash.to_string()
    );
    // C# GetRawTransaction verbose adds only blockhash, confirmations and blocktime
    // (RpcServer.Blockchain.cs:373-381) — NOT vmstate, which belongs to
    // getapplicationlog. Guard against re-introducing the non-C# field.
    assert!(
        obj.get("vmstate").is_none(),
        "getrawtransaction verbose must not include a vmstate field (C# parity)"
    );
    assert!(obj.get("confirmations").is_some());
    assert!(obj.get("blocktime").is_some());

    let bytes = BASE64_STANDARD
        .decode(
            (handler.callback())(
                &server,
                &[Value::String(tx.hash().to_string()), Value::Bool(false)],
            )
            .expect("base64 response")
            .as_str()
            .expect("base64"),
        )
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());
}

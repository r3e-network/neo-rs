//! Integration tests for the `validateaddress` RPC helper surface.
#![cfg(feature = "server")]

use neo_config::ProtocolSettings;
use neo_primitives::UInt160;
use neo_rpc::{RpcServer, RpcServerConfig, server::NodeContext};
use neo_system::Node;
use serde_json::Value;
use std::sync::Arc;

fn node_to_context(node: &Node) -> NodeContext {
    NodeContext::from_parts(
        Arc::clone(&node.settings),
        Arc::clone(&node.storage),
        node.blockchain.clone(),
        node.network.clone(),
        Arc::clone(&node.mempool),
        Arc::clone(&node.header_cache),
        node.services.clone(),
        Arc::clone(&node.native_contract_provider),
    )
}

fn is_valid(result: &Value) -> bool {
    result
        .get("isvalid")
        .and_then(Value::as_bool)
        .expect("validateaddress isvalid flag")
}

#[tokio::test(flavor = "multi_thread")]
async fn validate_address_uses_wallet_base58_check() {
    let node = Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("system");
    let system: Arc<NodeContext> = Arc::new(node_to_context(&node));
    let server = RpcServer::new(system, RpcServerConfig::default());

    let valid_address = UInt160::zero().to_address();
    assert!(is_valid(&server.validate_address(&valid_address)));

    let mut invalid_checksum = valid_address.clone();
    let last = invalid_checksum.pop().expect("address last char");
    invalid_checksum.push(if last == 'A' { 'B' } else { 'A' });
    assert!(!is_valid(&server.validate_address(&invalid_checksum)));

    let spaced = format!(" {valid_address} ");
    assert!(!is_valid(&server.validate_address(&spaced)));
}

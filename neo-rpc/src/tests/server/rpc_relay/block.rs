use super::*;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_payloads::Header;
use neo_primitives::UInt256;

#[tokio::test(flavor = "multi_thread")]
async fn relay_block_preflight_rejects_bad_merkle_root_as_invalid() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let mut header = Header::new();
    header.set_index(1);
    header.set_merkle_root(UInt256::from([0x42; 32]));
    let block = Block::from_parts(header, Vec::new());

    let result = relay_block(&server, block).expect("relay result");

    assert_eq!(result.inventory_type, InventoryType::Block);
    assert_eq!(result.block_index, Some(1));
    assert_eq!(result.result, VerifyResult::Invalid);
}

#[test]
fn relay_block_tip_read_uses_ledger_provider_boundary() {
    let source = include_str!("../../../server/rpc_relay/block.rs");
    let relay_start = source
        .find("pub(in crate::server) fn relay_block")
        .expect("relay_block exists");
    let relay = &source[relay_start..];

    assert!(
        relay.contains("StorageLedgerProviderFactory"),
        "RPC block relay tip reads must route through the ledger provider factory"
    );
    assert!(
        !relay.contains("LedgerContract::new()"),
        "RPC block relay must not construct native LedgerContract directly"
    );
}

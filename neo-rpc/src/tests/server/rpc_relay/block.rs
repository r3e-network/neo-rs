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
fn relay_block_tip_read_uses_relay_provider_boundary() {
    let block = include_str!("../../../server/rpc_relay/block.rs");
    let relay_start = block
        .find("pub(in crate::server) fn relay_block")
        .expect("relay_block exists");
    let relay = &block[relay_start..];

    assert!(
        relay.contains("NativeRelayLedgerProviderFactory"),
        "RPC block relay tip reads must route through the local relay provider factory"
    );
    assert!(
        !relay.contains("StorageLedgerProviderFactory"),
        "RPC block relay should not construct raw ledger providers directly"
    );
    assert!(
        !relay.contains("LedgerContract::new()"),
        "RPC block relay must not construct native LedgerContract directly"
    );

    let provider = include_str!("../../../server/rpc_relay/ledger_provider.rs");
    assert!(provider.contains("trait RelayLedgerProvider"));
    assert!(provider.contains("trait RelayLedgerProviderFactory"));
    assert!(provider.contains("struct NativeRelayLedgerProviderFactory"));
    assert!(
        provider.contains("ledger_queries::current_index"),
        "relay ledger provider should use the shared ledger-query boundary"
    );
    assert!(
        !provider.contains("StorageLedgerProviderFactory"),
        "relay ledger provider should not duplicate raw ledger provider construction"
    );
}

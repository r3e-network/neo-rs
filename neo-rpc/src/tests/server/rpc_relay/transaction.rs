#[test]
fn transaction_relay_uses_the_blockchain_service_boundary() {
    let source = include_str!("../../../server/rpc_relay/transaction.rs");
    let start = source
        .find("pub(in crate::server) fn relay_transaction")
        .expect("relay_transaction exists");
    let relay = &source[start..];

    assert!(
        relay.contains("blockchain.add_transaction(TransactionOrigin::Local, transaction)"),
        "RPC transaction relay must use BlockchainHandle admission"
    );
    assert!(
        !relay.contains(".submit_transaction("),
        "RPC transaction relay must not bypass the blockchain service"
    );
}

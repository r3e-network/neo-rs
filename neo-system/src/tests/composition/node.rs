use super::*;
use crate::NodeBuilder;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

#[test]
fn builder_returns_node_builder() {
    let _: NodeBuilder = Node::builder();
}

#[test]
fn tx_admission_uses_ledger_provider_boundary() {
    let source = include_str!("../../composition/node.rs");
    let start = source
        .find("pub fn try_enqueue_preverify")
        .expect("try_enqueue_preverify exists");
    let body = &source[start..];

    assert!(
        body.contains("StorageLedgerProviderFactory"),
        "composition-root tx admission must read ledger records through the provider factory"
    );
    assert!(
        !body.contains("LedgerContract::new()"),
        "composition-root tx admission must not construct native LedgerContract directly"
    );
}

#[tokio::test]
async fn cancellation_token_clone_is_independent() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .build()
        .expect("builder should succeed");

    let token = node.cancellation_token();
    token.cancel();
    assert!(node.shutdown.is_cancelled());
}

#[test]
fn direct_constructor_uses_builder_defaults() {
    let _guard = crate::composition::native_provider_test_guard();
    NativeContractLookup::replace_provider(None);

    let node = Node::new(Arc::new(ProtocolSettings::default()), None, None)
        .expect("headless node should use builder defaults");

    assert!(node.mempool.total_count() == 0);
    assert_eq!(node.header_cache.count(), 0);
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
    assert!(
        NativeContractLookup::native_contract_provider().is_none(),
        "Node::new should use a local provider through NodeBuilder defaults"
    );
}

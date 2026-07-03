use super::*;
use crate::NodeBuilder;
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

#[test]
fn builder_returns_node_builder() {
    let _: NodeBuilder = Node::builder();
}

#[tokio::test]
async fn cancellation_token_clone_is_independent() {
    // Building a node installs the process-global native contract provider; hold
    // the shared guard so this does not race the provider-asserting tests in
    // builder.rs (same neo-system test binary, parallel threads).
    let _guard = crate::composition::native_provider_test_guard();
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

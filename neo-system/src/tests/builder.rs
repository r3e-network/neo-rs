use super::*;
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

#[test]
fn builder_requires_settings() {
    let result = NodeBuilder::default().build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_storage() {
    let result = NodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_blockchain_and_network() {
    let result = NodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .with_storage(memory_store())
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_succeeds_with_required_services() {
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
        .expect("required services set");
    assert!(node.block_executor.is_none());
    assert!(node.consensus.is_none());
    assert!(node.engine.is_none());
}

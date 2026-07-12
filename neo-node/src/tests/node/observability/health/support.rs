use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_primitives::UInt256;
use neo_storage::persistence::providers::memory_store::MemoryStore;

use crate::node::services::NodeServiceHandles;

pub(super) fn test_node() -> neo_system::Node {
    neo_system::Node::new(Arc::new(ProtocolSettings::default()), None, None).expect("node")
}

pub(super) fn empty_services() -> Arc<NodeServiceHandles<MemoryStore>> {
    service_handles(None, None)
}

pub(super) fn service_handles(
    indexer: Option<Arc<neo_indexer::IndexerService>>,
    remote_ledger: Option<Arc<crate::node::remote_ledger::RemoteLedgerStatus>>,
) -> Arc<NodeServiceHandles<MemoryStore>> {
    Arc::new(NodeServiceHandles::new(
        None,
        None,
        indexer,
        None,
        None,
        remote_ledger,
    ))
}

pub(super) fn remote_ledger_node(
    height: u32,
) -> (neo_system::Node, Arc<NodeServiceHandles<MemoryStore>>) {
    remote_ledger_node_with_height(Some(height))
}

pub(super) fn remote_ledger_node_with_height(
    height: Option<u32>,
) -> (neo_system::Node, Arc<NodeServiceHandles<MemoryStore>>) {
    let node = test_node();
    let services = service_handles(
        None,
        Some(Arc::new(
            crate::node::remote_ledger::RemoteLedgerStatus::new(
                "https://rpc.example.invalid",
                height,
            ),
        )),
    );
    (node, services)
}

pub(super) fn remote_ledger_node_with_error(
    error: &str,
) -> (neo_system::Node, Arc<NodeServiceHandles<MemoryStore>>) {
    let node = test_node();
    let services = service_handles(
        None,
        Some(Arc::new(
            crate::node::remote_ledger::RemoteLedgerStatus::unavailable(
                "https://rpc.example.invalid",
                error,
            ),
        )),
    );
    (node, services)
}

pub(super) fn seed_ledger_height(node: &neo_system::Node, height: u32) {
    let pointer = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&UInt256::zero(), height)
        .expect("serialize current ledger pointer");
    let mut store = node.store_cache();
    store.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        neo_storage::StorageItem::from_bytes(pointer),
    );
    store
        .try_commit()
        .expect("commit health-test Ledger height");
}

pub(super) fn indexed_service_at(height: u32) -> Arc<neo_indexer::IndexerService> {
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let mut header = neo_payloads::Header::new();
    header.set_index(height);
    indexer
        .index_block(&neo_payloads::Block::from_parts(header, Vec::new()))
        .expect("index block");
    indexer
}

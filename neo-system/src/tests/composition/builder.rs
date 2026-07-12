use super::*;
use neo_runtime::{BlockOrigin, SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind};
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<MemoryStore> {
    Arc::new(MemoryStore::new())
}

fn native_provider() -> Arc<neo_native_contracts::StandardNativeProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

type StandardNodeBuilder = NodeBuilder<neo_native_contracts::StandardNativeProvider>;

#[test]
fn builder_requires_settings() {
    let result = StandardNodeBuilder::default().build();

    assert!(result.is_err());
}

#[test]
fn builder_requires_storage() {
    let result = StandardNodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_blockchain_and_network() {
    let result = StandardNodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .with_storage(memory_store())
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_native_contract_provider() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);

    let result = StandardNodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .build();

    assert!(result.is_err());
}

#[test]
fn builder_succeeds_with_required_services_and_native_provider() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let provider = native_provider();

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_native_contract_provider(Arc::clone(&provider))
        .build()
        .expect("required services set");
    assert!(Arc::ptr_eq(&node.native_contract_provider, &provider));
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );

    let pipeline = node.staged_sync_pipeline();
    assert!(Arc::ptr_eq(&pipeline, &node.staged_sync_pipeline));
    let import = pipeline.import();
    assert_eq!(import.origin(), BlockOrigin::Sync);
    assert!(
        import.import_queue().max_concurrency() >= 1,
        "sync import queue must bound preverification without stalling"
    );
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 12).with_counters(12, 512);
    import
        .checkpoint_store()
        .put_checkpoint(checkpoint.clone())
        .expect("default sync pipeline should persist checkpoints through node storage");
    assert_eq!(
        import
            .checkpoint_store()
            .checkpoint(SyncStageKind::Import)
            .expect("read checkpoint"),
        Some(checkpoint)
    );
}

#[test]
fn builder_keeps_custom_native_contract_provider_local() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let provider = native_provider();

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_native_contract_provider(Arc::clone(&provider))
        .build()
        .expect("required services set");

    assert!(Arc::ptr_eq(&node.native_contract_provider, &provider));
    assert!(
        Arc::ptr_eq(&node.mempool.native_contract_provider(), &provider),
        "default mempool should capture the same native provider as the composed node"
    );
}

#[test]
fn builder_keeps_custom_staged_sync_pipeline_local() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let pipeline = Arc::new(StagedSyncPipeline::new(
        bc.clone(),
        Arc::new(HeaderCache::new()),
        Arc::clone(&storage),
    ));
    let provider = native_provider();

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_native_contract_provider(provider)
        .with_staged_sync_pipeline(Arc::clone(&pipeline))
        .build()
        .expect("required services set");

    assert!(Arc::ptr_eq(&node.staged_sync_pipeline(), &pipeline));
    assert!(Arc::ptr_eq(&node.staged_sync_pipeline, &pipeline));
}

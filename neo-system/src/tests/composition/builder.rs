use super::*;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_runtime::{BlockOrigin, SyncStageCheckpoint, SyncStageKind};
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

// Shared with node.rs tests via the parent module, so tests that deliberately
// inspect the process-global native provider serialize on one lock.
fn native_provider_test_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::composition::native_provider_test_guard()
}

#[test]
fn builder_requires_settings() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

    let result = NodeBuilder::default().build();

    assert!(result.is_err());
    assert!(NativeContractLookup::native_contract_provider().is_none());
}

#[test]
fn builder_requires_storage() {
    // `.build()` can touch the process-global native contract provider, so take
    // the shared guard to stay serialized with the provider-asserting tests.
    let _guard = native_provider_test_lock();
    let result = NodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_blockchain_and_network() {
    let _guard = native_provider_test_lock();
    let result = NodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .with_storage(memory_store())
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_succeeds_with_required_services() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

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
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
    assert!(
        NativeContractLookup::native_contract_provider().is_none(),
        "NodeBuilder must keep the provider on the composed node instead of mutating the global bridge"
    );

    let pipeline = node.sync_import_pipeline();
    assert_eq!(pipeline.origin(), BlockOrigin::Sync);
    assert!(
        pipeline.import_queue().max_concurrency() >= 1,
        "sync import queue must bound preverification without stalling"
    );
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 12).with_counters(12, 512);
    pipeline
        .checkpoint_store()
        .put_checkpoint(checkpoint.clone())
        .expect("default sync pipeline should persist checkpoints through node storage");
    assert_eq!(
        pipeline
            .checkpoint_store()
            .checkpoint(SyncStageKind::Import)
            .expect("read checkpoint"),
        Some(checkpoint)
    );
}

#[test]
fn builder_keeps_custom_native_contract_provider_local() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let provider = Arc::new(neo_native_contracts::StandardNativeProvider::new())
        as Arc<dyn NativeContractProvider>;

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
        NativeContractLookup::native_contract_provider().is_none(),
        "custom providers should be captured by the node, not installed globally"
    );
}

#[test]
fn builder_keeps_custom_sync_import_pipeline_local() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let pipeline = Arc::new(SyncImportPipeline::new(bc.clone(), Arc::clone(&storage)));

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_sync_import_pipeline(Arc::clone(&pipeline))
        .build()
        .expect("required services set");

    assert!(Arc::ptr_eq(&node.sync_import_pipeline(), &pipeline));
}

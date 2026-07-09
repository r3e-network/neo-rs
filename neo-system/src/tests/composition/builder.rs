use super::*;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_runtime::{BlockOrigin, SyncStageCheckpoint, SyncStageKind};
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

fn native_provider() -> Arc<neo_native_contracts::StandardNativeProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

type StandardNodeBuilder = NodeBuilder<neo_native_contracts::StandardNativeProvider>;

// Shared with node.rs tests via the parent module, so tests that deliberately
// inspect the process-global native provider serialize on one lock.
fn native_provider_test_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::composition::native_provider_test_guard()
}

#[test]
fn builder_requires_settings() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

    let result = StandardNodeBuilder::default().build();

    assert!(result.is_err());
    assert!(NativeContractLookup::native_contract_provider().is_none());
}

#[test]
fn builder_requires_storage() {
    let _guard = native_provider_test_lock();
    let result = StandardNodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_blockchain_and_network() {
    let _guard = native_provider_test_lock();
    let result = StandardNodeBuilder::default()
        .with_settings(Arc::new(ProtocolSettings::default()))
        .with_storage(memory_store())
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_requires_native_contract_provider() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

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
    assert!(
        NativeContractLookup::native_contract_provider().is_none(),
        "NodeBuilder must not install a process-global fallback provider"
    );
}

#[test]
fn builder_succeeds_with_required_services_and_native_provider() {
    let _guard = native_provider_test_lock();
    NativeContractLookup::replace_provider(None);

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
    assert!(
        NativeContractLookup::native_contract_provider().is_none(),
        "NodeBuilder must keep the provider on the composed node instead of mutating the global bridge"
    );

    let pipeline = node.sync_import_pipeline();
    assert!(Arc::ptr_eq(
        &pipeline,
        &node
            .get_service::<SyncImportPipeline>()
            .expect("sync pipeline should be discoverable as a service")
    ));
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
    let provider = native_provider();

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_native_contract_provider(provider)
        .with_sync_import_pipeline(Arc::clone(&pipeline))
        .build()
        .expect("required services set");

    assert!(Arc::ptr_eq(&node.sync_import_pipeline(), &pipeline));
    assert!(Arc::ptr_eq(
        &node
            .get_service::<SyncImportPipeline>()
            .expect("custom sync pipeline should be registered"),
        &pipeline
    ));
}

#[test]
fn builder_uses_pre_registered_sync_import_pipeline_when_not_explicit() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let pipeline = Arc::new(SyncImportPipeline::new(bc.clone(), Arc::clone(&storage)));
    let services = ServiceRegistry::new();
    services.register(Arc::clone(&pipeline));
    let provider = native_provider();

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .with_native_contract_provider(provider)
        .with_services(services)
        .build()
        .expect("required services set");

    assert!(Arc::ptr_eq(&node.sync_import_pipeline(), &pipeline));
    assert!(Arc::ptr_eq(
        &node
            .get_service::<SyncImportPipeline>()
            .expect("pre-registered sync pipeline should remain discoverable"),
        &pipeline
    ));
}

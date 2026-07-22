use super::*;
use neo_runtime::{BlockOrigin, SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind};
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<MemoryStore> {
    Arc::new(MemoryStore::new())
}

fn native_provider() -> Arc<neo_native_contracts::StandardNativeProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

fn memory_pool(
    chain_spec: &Arc<neo_config::NeoChainSpec>,
    provider: &Arc<neo_native_contracts::StandardNativeProvider>,
) -> Arc<MemoryPool> {
    Arc::new(MemoryPool::new_with_native_contract_provider(
        Arc::clone(chain_spec),
        neo_mempool::TxPoolConfig::default(),
        Arc::clone(provider),
    ))
}

#[test]
fn required_components_are_constructor_arguments() {
    let source = include_str!("../../composition/builder.rs");

    assert!(!source.contains("chain_spec: Option"));
    assert!(!source.contains("storage: Option"));
    assert!(!source.contains("blockchain: Option"));
    assert!(!source.contains("network: Option"));
    assert!(!source.contains("mempool: Option"));
    assert!(!source.contains("native_contract_provider: Option"));
}

#[test]
fn builder_preserves_one_required_component_graph() {
    let storage = memory_store();
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec");
    let (blockchain, _commands) = BlockchainHandle::with_capacity();
    let (network, _network_commands, _network_events) = NetworkHandle::channel(8, 8);
    let header_cache = Arc::new(HeaderCache::new());
    let provider = native_provider();
    let mempool = memory_pool(&chain_spec, &provider);

    let node = NodeBuilder::new(
        Arc::clone(&chain_spec),
        Arc::clone(&storage),
        blockchain,
        network,
        Arc::clone(&mempool),
        Arc::clone(&header_cache),
        Arc::clone(&provider),
    )
    .build();

    assert!(Arc::ptr_eq(&node.chain_spec(), &chain_spec));
    assert!(Arc::ptr_eq(&node.storage(), &storage));
    assert!(Arc::ptr_eq(&node.mempool(), &mempool));
    assert!(Arc::ptr_eq(&node.header_cache(), &header_cache));
    assert!(Arc::ptr_eq(&node.native_contract_provider(), &provider));
    assert!(Arc::ptr_eq(
        &node.mempool().native_contract_provider(),
        &provider
    ));

    let pipeline = node.staged_sync_pipeline();
    assert_eq!(pipeline.import().origin(), BlockOrigin::Sync);
    assert!(Arc::ptr_eq(
        &pipeline.import().import_queue(),
        &node.live_block_import_pipeline().import_queue(),
    ));
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 12).with_counters(12, 512);
    pipeline
        .import()
        .checkpoint_store()
        .put_checkpoint(checkpoint.clone())
        .expect("store import checkpoint");
    assert_eq!(
        pipeline
            .import()
            .checkpoint_store()
            .checkpoint(SyncStageKind::Import)
            .expect("read import checkpoint"),
        Some(checkpoint)
    );
}

#[test]
fn builder_preserves_explicit_staged_sync_pipeline() {
    let storage = memory_store();
    let chain_spec = neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec");
    let (blockchain, _commands) = BlockchainHandle::with_capacity();
    let (network, _network_commands, _network_events) = NetworkHandle::channel(8, 8);
    let header_cache = Arc::new(HeaderCache::new());
    let pipeline = Arc::new(StagedSyncPipeline::new(
        blockchain.clone(),
        Arc::clone(&header_cache),
        Arc::clone(&storage),
    ));
    let provider = native_provider();
    let mempool = memory_pool(&chain_spec, &provider);

    let node = NodeBuilder::new(
        chain_spec,
        storage,
        blockchain,
        network,
        mempool,
        header_cache,
        provider,
    )
    .with_staged_sync_pipeline(Arc::clone(&pipeline))
    .build();

    assert!(Arc::ptr_eq(&node.staged_sync_pipeline(), &pipeline));
    assert!(Arc::ptr_eq(
        &pipeline.import().import_queue(),
        &node.live_block_import_pipeline().import_queue(),
    ));
}

use super::*;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_storage::persistence::providers::memory_store::MemoryStore;

static NATIVE_PROVIDER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

fn native_provider_test_lock() -> std::sync::MutexGuard<'static, ()> {
    NATIVE_PROVIDER_TEST_LOCK
        .lock()
        .expect("native provider test lock should not be poisoned")
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
    let _guard = native_provider_test_lock();
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
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
    assert!(NativeContractLookup::native_contract_provider().is_some());
}

#[test]
fn builder_installs_custom_native_contract_provider() {
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

    let installed =
        NativeContractLookup::native_contract_provider().expect("provider should be installed");
    assert!(Arc::ptr_eq(&node.native_contract_provider, &provider));
    assert!(Arc::ptr_eq(&installed, &provider));
}

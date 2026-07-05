use super::*;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

// Shared with node.rs's build test via the parent module, so ALL tests in the
// neo-system binary that touch the process-global native provider serialize on
// one lock (a per-file lock would not stop cross-module races).
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

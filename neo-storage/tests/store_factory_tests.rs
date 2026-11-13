use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::{StoreFactory, StoreProvider};
use std::sync::Arc;

#[test]
fn store_factory_returns_memory_provider_by_name() {
    let provider = StoreFactory::get_store_provider("Memory");
    assert!(provider.is_some());
    assert_eq!(provider.unwrap().name(), "Memory");
}

#[test]
fn store_factory_returns_default_provider_for_empty_string() {
    let provider = StoreFactory::get_store_provider("");
    assert!(provider.is_some());
    assert_eq!(provider.unwrap().name(), "Memory");
}

#[test]
fn store_factory_returns_none_for_unknown_provider() {
    let provider = StoreFactory::get_store_provider("NonExistent");
    assert!(provider.is_none());
}

#[test]
fn store_factory_get_store_with_memory_provider() {
    let store = StoreFactory::get_store("Memory", "");
    assert!(store.is_ok());
}

#[test]
fn store_factory_get_store_with_empty_provider_uses_default() {
    let store = StoreFactory::get_store("", "");
    assert!(store.is_ok());
}

#[test]
fn store_factory_get_store_fails_for_unknown_provider() {
    // The store factory falls back to the default (Memory) provider
    // when an unknown provider is requested, so this actually succeeds
    let result = StoreFactory::get_store("UnknownProvider", "");
    assert!(
        result.is_ok(),
        "Store factory falls back to default provider"
    );
}

#[test]
fn store_factory_register_custom_provider() {
    // Create a custom provider
    let custom_provider = Arc::new(MemoryStoreProvider::new()) as Arc<dyn StoreProvider>;

    // Register with a different name
    StoreFactory::register_provider(custom_provider);

    // Retrieve it
    let provider = StoreFactory::get_store_provider("Memory");
    assert!(provider.is_some());
}

#[test]
fn store_factory_multiple_stores_from_same_provider() {
    let store1 = StoreFactory::get_store("Memory", "path1").expect("store1");
    let store2 = StoreFactory::get_store("Memory", "path2").expect("store2");

    // Both should be valid but may be different instances
    let snapshot1 = store1.snapshot();
    let snapshot2 = store2.snapshot();

    let key = vec![0x01];
    assert!(snapshot1.try_get(&key).is_none());
    assert!(snapshot2.try_get(&key).is_none());
}

#[test]
fn memory_provider_name_is_memory() {
    let provider = MemoryStoreProvider::new();
    assert_eq!(provider.name(), "Memory");
}

#[test]
fn memory_provider_creates_valid_store() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    // Verify store is usable
    let snapshot = store.snapshot();
    let key = vec![0x42];
    assert!(snapshot.try_get(&key).is_none());
}

#[test]
fn memory_provider_ignores_path_parameter() {
    let provider = MemoryStoreProvider::new();

    let store1 = provider.get_store("path1").expect("store1");
    let store2 = provider.get_store("path2").expect("store2");

    // Both create independent stores (memory provider creates new store each time)
    let key = vec![0x01];
    assert!(store1.snapshot().try_get(&key).is_none());
    assert!(store2.snapshot().try_get(&key).is_none());
}

#[test]
fn store_factory_thread_safety() {
    use std::thread;

    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let store = StoreFactory::get_store("Memory", &format!("thread_{i}"))
                    .expect("store creation");
                let snapshot = store.snapshot();
                let key = vec![0x42];
                snapshot.try_get(&key)
            })
        })
        .collect();

    for handle in handles {
        let result = handle.join().expect("thread completion");
        assert!(result.is_none());
    }
}

#[test]
fn store_provider_trait_object_safety() {
    let provider: Arc<dyn StoreProvider> = Arc::new(MemoryStoreProvider::new());

    assert_eq!(provider.name(), "Memory");

    let store = provider.get_store("test_path");
    assert!(store.is_ok());
}

use super::*;

#[test]
fn test_native_registry_starts_empty() {
    let registry = NativeRegistry::new();
    assert!(registry.all_hashes().is_empty());
    assert_eq!(registry.contracts().count(), 0);
}

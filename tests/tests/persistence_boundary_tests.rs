use std::fs;
use std::path::Path;

#[test]
fn storage_types_have_one_implementation_and_compatibility_shims_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let storage_key =
        fs::read_to_string(root.join("neo-storage/src/types/storage_key.rs")).unwrap();
    let storage_item =
        fs::read_to_string(root.join("neo-storage/src/types/storage_item.rs")).unwrap();
    let key_builder =
        fs::read_to_string(root.join("neo-smart-contract/src/key_builder.rs")).unwrap();

    assert!(storage_key.contains("pub struct StorageKey"));
    assert!(storage_item.contains("pub struct StorageItem"));
    assert!(key_builder.contains("pub fn to_storage_key(&self) -> neo_storage::StorageKey"));
    assert!(!key_builder.contains("pub struct StorageKey"));
    assert!(!root.join("neo-core").exists());
}

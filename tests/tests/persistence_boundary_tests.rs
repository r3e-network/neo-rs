use std::fs;
use std::path::Path;

#[test]
fn storage_types_have_one_implementation_and_compatibility_shims_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let storage_key =
        fs::read_to_string(root.join("neo-storage/src/types/storage_key.rs")).unwrap();
    let storage_item = fs::read_to_string(root.join("neo-storage/src/types/track.rs")).unwrap();
    let persistence_key =
        fs::read_to_string(root.join("neo-core/src/persistence/storage_key.rs")).unwrap();
    let smart_key =
        fs::read_to_string(root.join("neo-core/src/smart_contract/storage_key.rs")).unwrap();

    assert!(storage_key.contains("pub struct StorageKey"));
    assert!(!storage_item.contains("struct StorageItem"));
    assert!(persistence_key.contains("pub use neo_storage::StorageKey;"));
    assert!(smart_key.contains("pub use crate::persistence::storage_key::*;"));
    assert!(!persistence_key.contains("pub struct StorageKey"));
    assert!(!smart_key.contains("pub struct StorageKey"));
}

use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn opens_store_and_creates_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider
        .get_store(db_path.to_str().unwrap())
        .expect("rocksdb store");
    assert!(db_path.exists(), "db path should be created");

    // basic snapshot call to ensure the store is usable
    let _snapshot = store.get_snapshot();
}

#[test]
fn returns_error_when_path_is_file() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("not-a-dir");
    fs::write(&file_path, b"content").expect("write file");

    let cfg = StorageConfig {
        path: file_path.clone(),
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(cfg);

    let result = provider.get_store(file_path.to_str().unwrap());
    match result {
        Ok(_) => panic!("expected failure when path is a file"),
        Err(err) => {
            assert!(
                err.to_string()
                    .to_ascii_lowercase()
                    .contains("failed to open rocksdb store"),
                "unexpected error: {err}"
            );
        }
    }
}

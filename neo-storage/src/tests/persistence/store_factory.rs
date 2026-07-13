use super::{StoreFactory, StoreProviderKind};
use crate::persistence::storage::StorageConfig;

#[test]
fn built_in_provider_names_are_registered() {
    assert!(StoreFactory::get_store_provider("").is_none());
    assert_eq!(
        StoreFactory::get_store_provider("memory"),
        Some(StoreProviderKind::Memory)
    );
    assert!(StoreFactory::get_store_provider("Memory").is_some());
    assert!(
        StoreFactory::get_store_provider("RocksDBStore").is_none(),
        "removed backends must not remain in the production provider contract"
    );
    assert_eq!(
        StoreFactory::get_store_provider("mdbx"),
        Some(StoreProviderKind::Mdbx)
    );
    assert!(
        StoreFactory::get_store_provider("MdbxStore").is_none(),
        "legacy concrete-type aliases are not part of the production provider contract"
    );
}

#[test]
fn unknown_provider_is_rejected_instead_of_falling_back_to_memory() {
    let err = match StoreFactory::get_store("typoed-backend", "") {
        Ok(_) => panic!("unknown storage provider must be rejected"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("typoed-backend"),
        "error should name the unknown provider: {err}"
    );
}

#[test]
fn empty_provider_is_rejected_instead_of_falling_back_to_memory() {
    let err = match StoreFactory::get_store("", "") {
        Ok(_) => panic!("empty storage provider must be explicit"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("empty storage provider"),
        "error should explain the explicit-provider requirement: {err}"
    );
}

#[test]
fn memory_provider_name_creates_memory_store() {
    let memory_store = StoreFactory::get_store("memory", "").expect("memory store");

    assert!(memory_store.as_memory().is_some());
}

#[cfg(unix)]
#[test]
fn factory_accepts_non_utf8_paths_without_string_conversion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let non_utf8_path = temp
        .path()
        .join(OsString::from_vec(vec![b'n', b'e', b'o', 0xFF]));
    let memory_store = StoreFactory::get_store("memory", &non_utf8_path).expect("memory store");

    assert!(memory_store.as_memory().is_some());
}

#[test]
fn removed_rocksdb_provider_is_rejected() {
    let err = StoreFactory::get_store("rocksdb", "")
        .expect_err("removed RocksDB provider must be rejected");

    assert!(err.to_string().contains("rocksdb"));
    assert!(err.to_string().contains("memory, mdbx"));
}

#[test]
fn mdbx_provider_name_creates_mdbx_store() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("mdbx");
    let store = StoreFactory::get_store("mdbx", &path).expect("mdbx store");

    assert!(store.as_mdbx().is_some());
}

#[test]
fn default_build_registers_mdbx_provider_for_production_storage() {
    let provider = StoreFactory::get_store_provider("mdbx")
        .expect("default neo-storage build must register MDBX");

    assert_eq!(provider, StoreProviderKind::Mdbx);
}

#[test]
fn configured_mdbx_provider_preserves_read_only_flag() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("missing-read-only-store");
    let err = match StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path,
            read_only: true,
            ..StorageConfig::default()
        },
    ) {
        Ok(_) => panic!("read-only MDBX open must not create a missing store"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("failed to open MDBX store"),
        "unexpected error: {err}"
    );
}

#[test]
fn configured_mdbx_provider_uses_supplied_geometry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("configured-mdbx");
    let map_size = 128 * 1024 * 1024;

    let store = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path,
            mdbx_geometry_upper_bytes: Some(map_size),
            ..StorageConfig::default()
        },
    )
    .expect("configured MDBX store");
    let store = store.as_mdbx().expect("MDBX store");

    assert_eq!(
        store.info().expect("MDBX info").map_size(),
        map_size as usize,
        "factory must pass MDBX geometry settings from StorageConfig to the provider"
    );
}

#[test]
fn mdbx_provider_reads_tuning_from_storage_config() {
    let provider = crate::mdbx::MdbxStoreProvider::new(StorageConfig {
        mdbx_geometry_upper_bytes: Some(1024 * 1024 * 1024),
        mdbx_geometry_growth_bytes: Some(16 * 1024 * 1024),
        mdbx_max_readers: Some(128),
        ..StorageConfig::default()
    });
    let tuning = provider.tuning();

    assert_eq!(tuning.map_size, 1024 * 1024 * 1024);
    assert_eq!(tuning.growth_step, 16 * 1024 * 1024);
    assert_eq!(
        tuning.max_readers, 128,
        "provider should retain requested MDBX max readers for wrapper-level enforcement"
    );
}

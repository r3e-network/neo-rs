use super::{
    providers::memory_store_provider::MemoryStoreProvider, storage::StorageConfig, store::Store,
    store_provider::StoreProvider,
};
use crate::error::{StorageError, StorageResult};
use crate::mdbx::MdbxStoreProvider;
use crate::rocksdb::RocksDBStoreProvider;
use hashbrown::HashMap;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;

const MEMORY_PROVIDER: &str = "memory";
const ROCKSDB_PROVIDER: &str = "rocksdb";
const MDBX_PROVIDER: &str = "mdbx";

/// Global registry of store providers.
static PROVIDERS: LazyLock<RwLock<HashMap<String, Arc<dyn StoreProvider>>>> = LazyLock::new(|| {
    let mut providers = HashMap::new();

    let mem_provider = Arc::new(MemoryStoreProvider::new()) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, MEMORY_PROVIDER, mem_provider);

    let rocksdb_provider =
        Arc::new(RocksDBStoreProvider::new(StorageConfig::default())) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, ROCKSDB_PROVIDER, rocksdb_provider);

    let mdbx_provider =
        Arc::new(MdbxStoreProvider::new(StorageConfig::default())) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, MDBX_PROVIDER, mdbx_provider);

    RwLock::new(providers)
});

/// Registry-backed facade for creating stores from named providers.
///
/// This is the only production entry point for opening storage backends by
/// name. Concrete backends implement [`StoreProvider`]; callers ask this facade
/// for `memory`, `mdbx`, or `rocksdb` stores instead of constructing backend
/// adapters directly.
pub struct StoreFactory;

impl StoreFactory {
    /// Register a store provider.
    pub fn register_provider(provider: Arc<dyn StoreProvider>) {
        let mut providers = PROVIDERS.write();
        providers.insert(provider_key(provider.name()), provider);
    }

    /// Get store provider by name.
    pub fn get_store_provider(name: &str) -> Option<Arc<dyn StoreProvider>> {
        if provider_key(name).is_empty() {
            return None;
        }
        let providers = PROVIDERS.read();
        providers.get(&provider_key(name)).cloned()
    }

    /// Creates a store through an explicitly named provider.
    ///
    /// # Arguments
    /// * `storage_provider` - The storage engine used to create the Store objects.
    ///   Empty names are rejected so production callers cannot accidentally
    ///   fall back to an ephemeral in-memory store.
    /// * `path` - The path used by persistent stores. In-memory stores ignore it.
    pub fn get_store<P>(storage_provider: &str, path: P) -> StorageResult<Arc<dyn Store>>
    where
        P: AsRef<Path>,
    {
        provider_for(storage_provider)?.get_store(path.as_ref())
    }

    /// Get store from a named provider and full storage configuration.
    ///
    /// This keeps callers on the provider/factory path when they need backend
    /// configuration beyond a path, such as read-only mode or cache settings.
    pub fn get_store_with_config(
        storage_provider: &str,
        config: StorageConfig,
    ) -> StorageResult<Arc<dyn Store>> {
        provider_for(storage_provider)?.get_store_with_config(config)
    }
}

fn empty_provider_error() -> StorageError {
    StorageError::invalid_operation(
        "empty storage provider is not supported; choose mdbx, rocksdb, or memory explicitly",
    )
}

fn provider_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn register_builtin_provider(
    providers: &mut HashMap<String, Arc<dyn StoreProvider>>,
    name: &str,
    provider: Arc<dyn StoreProvider>,
) {
    providers.insert(provider_key(name), provider);
}

fn provider_for(storage_provider: &str) -> StorageResult<Arc<dyn StoreProvider>> {
    let key = provider_key(storage_provider);
    if key.is_empty() {
        return Err(empty_provider_error());
    }
    let providers = PROVIDERS.read();
    providers
        .get(&key)
        .cloned()
        .ok_or_else(|| unknown_provider_error(storage_provider, &providers))
}

fn unknown_provider_error(
    requested: &str,
    providers: &HashMap<String, Arc<dyn StoreProvider>>,
) -> StorageError {
    let mut available = providers
        .keys()
        .filter(|name| !name.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    available.sort_unstable();
    available.dedup();

    StorageError::invalid_operation(format!(
        "Store provider {requested:?} not found; available providers: {}",
        available.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::StoreFactory;
    use crate::persistence::providers::MemoryStore;
    use crate::persistence::storage::StorageConfig;
    use crate::rocksdb::RocksDbStore;

    #[test]
    fn built_in_provider_names_are_registered() {
        assert!(StoreFactory::get_store_provider("").is_none());
        assert!(
            StoreFactory::get_store_provider("memory")
                .expect("memory provider")
                .as_any()
                .is::<crate::persistence::providers::MemoryStoreProvider>()
        );
        assert!(StoreFactory::get_store_provider("Memory").is_some());
        assert!(
            StoreFactory::get_store_provider("rocksdb")
                .expect("rocksdb provider")
                .as_any()
                .is::<crate::rocksdb::RocksDBStoreProvider>()
        );
        assert!(
            StoreFactory::get_store_provider("RocksDBStore").is_none(),
            "legacy concrete-type aliases are not part of the production provider contract"
        );
        assert!(
            StoreFactory::get_store_provider("mdbx")
                .expect("mdbx provider")
                .as_any()
                .is::<crate::mdbx::MdbxStoreProvider>()
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

        assert!(memory_store.as_any().is::<MemoryStore>());
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

        assert!(memory_store.as_any().is::<MemoryStore>());
    }

    #[test]
    fn rocksdb_provider_name_creates_rocksdb_store() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("rocksdb");
        let store = StoreFactory::get_store("rocksdb", &path).expect("rocksdb store");

        assert!(store.as_any().is::<RocksDbStore>());
    }

    #[test]
    fn configured_rocksdb_provider_preserves_read_only_flag() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("missing-read-only-store");
        let err = match StoreFactory::get_store_with_config(
            "rocksdb",
            StorageConfig {
                path,
                read_only: true,
                ..StorageConfig::default()
            },
        ) {
            Ok(_) => panic!("read-only RocksDB open must not create a missing store"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("failed to open RocksDB store"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn mdbx_provider_name_creates_mdbx_store() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("mdbx");
        let store = StoreFactory::get_store("mdbx", &path).expect("mdbx store");

        assert!(store.as_any().is::<crate::mdbx::MdbxStore>());
    }

    #[test]
    fn default_build_registers_mdbx_provider_for_production_storage() {
        let provider = StoreFactory::get_store_provider("mdbx")
            .expect("default neo-storage build must register MDBX");

        assert!(provider.as_any().is::<crate::mdbx::MdbxStoreProvider>());
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
        let store = store
            .as_any()
            .downcast_ref::<crate::mdbx::MdbxStore>()
            .expect("MDBX store");

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
}

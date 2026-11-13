//! Configuration validation and storage management for neo-node.
//!
//! This module provides production-ready configuration validation, storage
//! access checks, and provider selection logic.

#![allow(clippy::collapsible_if)] // Nested checks keep validation failures explicit.

use anyhow::{Context, Result};
use neo_protocol::ProtocolSettings;
use neo_storage::persistence::StoreProvider;
use neo_storage::persistence::storage::StorageConfig;
use neo_system::state_service::StateServiceSettings;
use std::path::Path;
use tracing::{info, warn};

#[cfg(feature = "full")]
use neo_storage::persistence::providers::RocksDBStoreProvider;
#[cfg(feature = "full")]
use neo_storage::persistence::providers::rocksdb::BatchCommitConfig;

/// Storage version identifier used for compatibility checking.
///
/// When storage format changes, this version is incremented to prevent
/// incompatible data from being loaded.
pub(crate) const STORAGE_VERSION: u32 = 1;

/// Network identifier file name for storage validation.
const NETWORK_METADATA_FILE: &str = "network.meta";

/// Storage version file name.
const VERSION_METADATA_FILE: &str = "version.meta";

/// Returns a human-readable build/feature summary string.
pub(crate) fn build_feature_summary() -> String {
    let features = [
        Some("base"),
        #[cfg(feature = "full")]
        Some("full"),
        #[cfg(feature = "tee")]
        Some("tee"),
        #[cfg(feature = "hsm")]
        Some("hsm"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    format!("features=[{}]", features.join(","))
}

/// Builds state service settings from configuration with full validation.
///
/// This validates all configuration options and ensures consistency with
/// protocol settings and CLI overrides.
pub(crate) fn build_state_service_settings(
    config: &crate::config::NodeConfig,
    storage_path: Option<&str>,
    protocol_settings: &ProtocolSettings,
) -> Result<Option<StateServiceSettings>> {
    let Some(mut settings) = config.state_service_settings(protocol_settings)? else {
        info!("State service not configured");
        return Ok(None);
    };

    // The effective storage path already includes the CLI override.
    if let Some(cli_path) = storage_path {
        settings.path = cli_path.to_string();
    }
    if settings.path.trim().is_empty() {
        anyhow::bail!("State service path cannot be empty");
    }

    if settings.network != protocol_settings.network {
        warn!(
            "State service network ({}) differs from protocol network ({})",
            settings.network, protocol_settings.network
        );
    }

    if settings.max_find_result_items == 0 {
        anyhow::bail!("max_find_result_items must be greater than 0");
    }
    if settings.max_find_result_items > 10000 {
        warn!(
            "max_find_result_items ({}) is very large, may impact performance",
            settings.max_find_result_items
        );
    }

    info!(
        "State service configured: full_state={}, auto_verify={}, max_results={}",
        settings.full_state, settings.auto_verify, settings.max_find_result_items
    );

    Ok(Some(settings))
}

/// Checks if storage path is accessible with proper permissions.
///
/// This performs comprehensive checks:
/// - Path existence and directory validation
/// - Write permissions verification
/// - Backend-specific health checks
/// - Disk space availability
pub(crate) fn check_storage_access<T>(
    backend: Option<&str>,
    path: Option<&str>,
    _config: T,
) -> Result<()> {
    // If no path specified, nothing to check
    let Some(path_str) = path else {
        return Ok(());
    };

    let path = Path::new(path_str);

    // Check if path exists
    if path.exists() {
        // Validate it's a directory
        if !path.is_dir() {
            anyhow::bail!("Storage path exists but is not a directory: {}", path_str);
        }

        // Check read permissions
        if let Err(e) = std::fs::read_dir(path) {
            anyhow::bail!("Storage directory is not readable: {} - {}", path_str, e);
        }

        // Check write permissions with actual file operation
        let test_file = path.join(".write_test");
        if let Err(e) = std::fs::write(&test_file, b"test") {
            anyhow::bail!("Storage directory is not writable: {} - {}", path_str, e);
        }

        // Clean up test file
        if let Err(e) = std::fs::remove_file(&test_file) {
            warn!("Failed to remove test file: {}", e);
        }

        info!("Storage path validated: {}", path_str);
    } else {
        // Try to create directory with parents
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create storage directory: {}", path_str))?;

        info!("Created storage directory: {}", path_str);
    }

    // Backend-specific health checks. LevelDB is deliberately absent: no
    // LevelDB StoreProvider implementation exists in neo-storage, so it is
    // not an accepted backend (see `select_store_provider`).
    if let Some(backend_name) = backend {
        match backend_name {
            "rocksdb" => {
                // Check for lock files
                let lock_file = path.join("LOCK");
                if lock_file.exists() {
                    warn!(
                        "Database lock file exists at {}. If no other process is using it, \
                         the previous session may have crashed. Remove the lock file manually \
                         or let the database recover automatically.",
                        lock_file.display()
                    );
                }

                // Check for corruption indicators
                let current_file = path.join("CURRENT");
                if current_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&current_file) {
                        if content.trim().is_empty() {
                            warn!("CURRENT file is empty, database may be corrupted");
                        }
                    }
                }
            }
            "memory" => {
                // Memory backend doesn't persist to disk
                warn!("Using memory backend - all data will be lost on restart");
            }
            _ => {}
        }
    }

    // Check available disk space (basic check)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let available = metadata.blocks() * 512; // Approximate
            if available < 1_000_000_000 {
                // Less than 1GB
                warn!("Low disk space available: ~{} bytes", available);
            }
        }
    }

    Ok(())
}

/// Validates that storage network matches protocol settings.
///
/// This ensures that a storage directory is not accidentally used with
/// a different network, which would cause consensus failures.
pub(crate) fn check_storage_network(
    path: &str,
    expected_network: u32,
    read_only: bool,
) -> Result<()> {
    let path = Path::new(path);

    // If storage doesn't exist, nothing to validate
    if !path.exists() {
        return Ok(());
    }

    let metadata_file = path.join(NETWORK_METADATA_FILE);

    if metadata_file.exists() {
        // Read and validate existing network
        let content = std::fs::read_to_string(&metadata_file)
            .with_context(|| format!("Failed to read network metadata from {}", path.display()))?;

        let stored_network: u32 = content
            .trim()
            .parse()
            .with_context(|| format!("Invalid network metadata in {}", metadata_file.display()))?;

        if stored_network != expected_network {
            anyhow::bail!(
                "Storage network mismatch at {}: expected network {}, found network {}. \
                 This storage directory belongs to a different network. \
                 Use --storage-path to specify a different location or --network to match the stored network.",
                path.display(),
                expected_network,
                stored_network
            );
        }

        info!("Storage network validated: network {}", expected_network);
    } else if !read_only {
        // Write network identifier for future validation
        std::fs::write(&metadata_file, expected_network.to_string()).with_context(|| {
            format!(
                "Failed to write network metadata to {}",
                metadata_file.display()
            )
        })?;

        info!(
            "Initialized storage network metadata: network {}",
            expected_network
        );
    }

    // Also check storage version
    let version_file = path.join(VERSION_METADATA_FILE);
    if version_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&version_file) {
            if let Ok(stored_version) = content.trim().parse::<u32>() {
                if stored_version != STORAGE_VERSION {
                    warn!(
                        "Storage version mismatch: expected {}, found {}. Migration may be required.",
                        STORAGE_VERSION, stored_version
                    );
                }
            }
        }
    } else if !read_only {
        if let Err(e) = std::fs::write(&version_file, STORAGE_VERSION.to_string()) {
            warn!("Failed to write version metadata: {}", e);
        }
    }

    Ok(())
}

/// Selects the storage provider for the requested backend, failing closed on
/// anything that cannot be honored exactly.
///
/// # Support matrix (verified against neo-storage)
///
/// | Backend   | Availability                    | Behavior |
/// |-----------|---------------------------------|----------|
/// | `memory`  | always                          | Returns `Ok(None)`, the documented delegation signal: `NeoSystem` construction falls back to its built-in default provider, which is the real `MemoryStoreProvider` registered in the `StoreFactory`. A memory store is ephemeral and needs no data path. |
/// | `rocksdb` | requires the `full` feature     | Constructs a real [`RocksDBStoreProvider`] from the supplied [`StorageConfig`] (including the RocksDB write-batch profile selected via `NEO_ROCKSDB_BATCH_PROFILE`). |
/// | `leveldb` | never                           | No LevelDB `StoreProvider` implementation exists in neo-storage, so the option fails closed instead of silently degrading to a memory store. |
///
/// Fail-closed policy: an unavailable backend is a hard error. This function
/// never silently substitutes a different backend than the one requested,
/// because a node that asked for persistent storage but quietly received an
/// ephemeral one loses all chain data on restart.
pub(crate) fn select_store_provider(
    backend: Option<&str>,
    storage_config: StorageConfig,
) -> Result<Option<Box<dyn StoreProvider>>> {
    let backend_name = backend.unwrap_or("memory");
    #[cfg(not(feature = "full"))]
    let _ = &storage_config;

    match backend_name {
        "memory" | "" => {
            info!("Selected memory storage backend (ephemeral, no persistence)");
            // `None` is the delegation contract, not a silent skip: NeoSystem
            // construction resolves `None` to the `MemoryStoreProvider` that
            // `StoreFactory` registers by default.
            Ok(None)
        }
        #[cfg(feature = "full")]
        "rocksdb" => {
            let (batch_config, batch_profile) = rocksdb_batch_profile_from_env();
            info!(
                path = %storage_config.path.display(),
                batch_profile,
                "Selected RocksDB storage backend; constructing real provider"
            );
            let provider =
                RocksDBStoreProvider::new(storage_config).with_batch_config(batch_config);
            Ok(Some(Box::new(provider)))
        }
        #[cfg(not(feature = "full"))]
        "rocksdb" => {
            anyhow::bail!(
                "storage backend 'rocksdb' is unavailable: this binary was built without the \
                 'full' feature, so the neo-storage RocksDB provider is not compiled in. \
                 Rebuild with `--features full` or select the 'memory' backend; refusing to \
                 silently fall back to an ephemeral memory store"
            );
        }
        "leveldb" => {
            anyhow::bail!(
                "storage backend 'leveldb' is not supported: neo-storage ships no LevelDB \
                 StoreProvider implementation. Supported backends: memory, rocksdb \
                 (requires the 'full' feature)"
            );
        }
        other => {
            anyhow::bail!(
                "unknown storage backend: '{}'. Supported backends: memory, rocksdb \
                 (requires the 'full' feature)",
                other
            );
        }
    }
}

/// Resolves the RocksDB write-batch commit profile from the
/// `NEO_ROCKSDB_BATCH_PROFILE` environment variable, returning the profile
/// and its human-readable name for logging.
///
/// Recognized values: `balanced`, `durable` (default when unset), and
/// `high_throughput` (also accepted as `high-throughput`, as used by the
/// operations scripts). Unknown values log a warning and fall back to the
/// durable profile rather than guessing at commit semantics.
#[cfg(feature = "full")]
fn rocksdb_batch_profile_from_env() -> (BatchCommitConfig, &'static str) {
    match std::env::var("NEO_ROCKSDB_BATCH_PROFILE").as_deref() {
        Ok("balanced") => (BatchCommitConfig::balanced(), "balanced"),
        Ok("high_throughput" | "high-throughput") => {
            (BatchCommitConfig::high_throughput(), "high_throughput")
        }
        Ok("durable") | Err(_) => (BatchCommitConfig::durable(), "durable"),
        Ok(other) => {
            warn!(
                value = other,
                "unknown NEO_ROCKSDB_BATCH_PROFILE; falling back to the 'durable' profile"
            );
            (BatchCommitConfig::durable(), "durable")
        }
    }
}

/// Validates node configuration for consistency and correctness.
///
/// This performs comprehensive validation of the entire node configuration,
/// checking for conflicts, missing required values, and security issues.
pub(crate) fn validate_node_config(
    config: &crate::config::NodeConfig,
    storage_path: Option<&str>,
    backend: Option<&str>,
    protocol_settings: &ProtocolSettings,
    rpc_hardened: bool,
) -> Result<()> {
    info!("Validating node configuration");

    config
        .execution
        .mode
        .parse::<neo_system::ExecutionMode>()
        .map_err(anyhow::Error::msg)
        .context("invalid execution.mode")?;

    // Validate network consistency across all plugins
    let expected_network = protocol_settings.network;

    if let Some(ref state_service) = config.state_service {
        if let Some(network) = state_service.network {
            if network != expected_network {
                anyhow::bail!(
                    "State service network ({}) doesn't match protocol network ({})",
                    network,
                    expected_network
                );
            }
        }
    }

    if let Some(ref app_logs) = config.application_logs {
        if let Some(network) = app_logs.network {
            if network != expected_network {
                anyhow::bail!(
                    "Application logs network ({}) doesn't match protocol network ({})",
                    network,
                    expected_network
                );
            }
        }
    }

    if let Some(ref tokens_tracker) = config.tokens_tracker {
        if let Some(network) = tokens_tracker.network {
            if network != expected_network {
                anyhow::bail!(
                    "Tokens tracker network ({}) doesn't match protocol network ({})",
                    network,
                    expected_network
                );
            }
        }
    }

    // Validate RPC configuration
    let rpc_config = &config.rpc;
    if rpc_hardened {
        info!("RPC server running in hardened mode (sensitive methods restricted)");
    }

    // Validate bind address
    if let Some(ref bind_addr) = rpc_config.bind_address {
        if bind_addr.starts_with("0.0.0.0") && !rpc_hardened {
            warn!(
                "RPC server binding to all interfaces ({}). Consider using 127.0.0.1 \
                 or enable --rpc-hardened for production.",
                bind_addr
            );
        }
    }

    // Validate max connections
    if let Some(max_conn) = rpc_config.max_connections {
        if max_conn == 0 {
            anyhow::bail!("RPC max_connections must be greater than 0");
        }
        if max_conn > 10000 {
            warn!(
                "RPC max_connections ({}) is very high, may impact performance",
                max_conn
            );
        }
    }

    // Validate storage configuration
    if storage_path.is_some() && backend.is_none() {
        warn!("Storage path specified but no backend selected, using default (memory)");
    }

    if backend.is_some() && storage_path.is_none() {
        warn!("Storage backend specified but no path given, using default location");
    }

    // Validate consensus configuration
    if let Some(ref dbft) = config.dbft {
        if dbft.auto_start.unwrap_or(false) {
            if config
                .unlock_wallet
                .path
                .as_ref()
                .is_none_or(|p| p.is_empty())
            {
                anyhow::bail!(
                    "Consensus auto-start enabled but no wallet unlock configuration found. \
                     Either configure 'UnlockWallet' section in config or disable 'AutoStart' in dBFT settings."
                );
            }

            info!("Consensus will start automatically with unlocked wallet");
        }

        // Validate block size limits
        if let Some(max_block_size) = dbft.max_block_size {
            if max_block_size == 0 {
                anyhow::bail!("dBFT max_block_size must be greater than 0");
            }
            if max_block_size > 10_000_000 {
                // 10MB
                warn!(
                    "dBFT max_block_size ({}) exceeds recommended maximum (10MB)",
                    max_block_size
                );
            }
        }
    }

    // Validate oracle service configuration
    if let Some(ref oracle) = config.oracle_service {
        if oracle.auto_start.unwrap_or(false)
            && config
                .unlock_wallet
                .path
                .as_ref()
                .is_none_or(|p| p.is_empty())
        {
            anyhow::bail!(
                "Oracle service auto-start enabled but no wallet unlock configuration found."
            );
        }

        if oracle.nodes.is_empty() {
            warn!("Oracle service configured but no oracle nodes specified");
        }

        // Validate timeouts
        if let Some(timeout) = oracle.max_task_timeout {
            if timeout == 0 {
                anyhow::bail!("Oracle max_task_timeout must be greater than 0");
            }
            if timeout > 300000 {
                // 5 minutes
                warn!("Oracle max_task_timeout ({} ms) is very long", timeout);
            }
        }
    }

    // Validate wallet unlock configuration
    if let Some(ref wallet_path_str) = config.unlock_wallet.path {
        if !wallet_path_str.is_empty() {
            let wallet_path = Path::new(wallet_path_str);
            if !wallet_path.exists() {
                warn!(
                    "Wallet file does not exist: {}. Will fail at runtime if needed.",
                    wallet_path_str
                );
            }
        }
    }

    info!("Node configuration validated successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::providers::MemoryStoreProvider;
    use neo_storage::persistence::store::Store;
    use neo_storage::types::StorageKey;
    use neo_system::NeoSystem;
    use std::path::PathBuf;

    fn test_storage_config(path: PathBuf) -> StorageConfig {
        StorageConfig {
            path,
            ..StorageConfig::default()
        }
    }

    /// Exercises a concrete store behind `Arc<dyn Store>` with a write/read
    /// round-trip so the assertions verify real backend behavior, not just
    /// that a provider object was returned.
    ///
    /// Writes go through `Arc::get_mut`, which is valid here because the
    /// store was just created and holds a sole reference.
    fn assert_store_roundtrip(store: std::sync::Arc<dyn Store>) {
        let mut store = store;
        let key = StorageKey::new(11, b"t11-probe".to_vec());
        {
            let writer =
                std::sync::Arc::get_mut(&mut store).expect("sole store reference for write access");
            writer
                .put(key.to_array(), b"t11-value".to_vec())
                .expect("store put must succeed");
        }
        let value = store
            .try_get(&key)
            .expect("written key must be readable")
            .to_value();
        assert_eq!(value, b"t11-value".to_vec());
    }

    #[test]
    fn memory_backend_delegates_to_default_provider() {
        // The memory contract: Ok(None) is the documented delegation signal;
        // NeoSystem construction resolves it to the MemoryStoreProvider that
        // StoreFactory registers by default.
        let provider = select_store_provider(
            Some("memory"),
            test_storage_config(PathBuf::from("./irrelevant")),
        )
        .expect("memory backend selection must succeed");
        assert!(provider.is_none());

        // No backend configured defaults to memory as well.
        let provider =
            select_store_provider(None, test_storage_config(PathBuf::from("./irrelevant")))
                .expect("default backend selection must succeed");
        assert!(provider.is_none());
    }

    #[test]
    fn memory_delegation_target_constructs_real_store() {
        // Verifies the delegation target is a real, working provider rather
        // than a stub, so the Ok(None) contract cannot mask a missing backend.
        let provider = MemoryStoreProvider::new();
        assert_eq!(provider.name(), "Memory");
        let store = provider
            .get_store("")
            .expect("memory store construction must succeed");
        assert_store_roundtrip(store);
    }

    #[cfg(feature = "full")]
    #[test]
    fn rocksdb_backend_constructs_real_persistent_provider() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = test_storage_config(dir.path().to_path_buf());
        let provider = select_store_provider(Some("rocksdb"), config)
            .expect("rocksdb backend must select with the full feature")
            .expect("rocksdb backend must construct a concrete provider");
        assert_eq!(provider.name(), "RocksDBStore");
        let store = provider
            .get_store(dir.path().to_str().expect("utf-8 temp path"))
            .expect("rocksdb store construction must succeed");
        assert_store_roundtrip(store);
        // The store is dropped before the temp dir (reverse declaration order),
        // which is required on Windows to release the database files.
    }

    #[cfg(feature = "full")]
    #[tokio::test(flavor = "multi_thread")]
    async fn rocksdb_system_persists_and_rehydrates_genesis_across_restart() {
        use neo_native_contracts::ledger_contract::LedgerContract;
        use neo_storage::persistence::StoreCache;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().to_str().expect("utf-8 temp path").to_string();
        let expected_genesis_hash = {
            let provider = select_store_provider(
                Some("rocksdb"),
                test_storage_config(dir.path().to_path_buf()),
            )
            .expect("select RocksDB")
            .expect("concrete RocksDB provider");
            let system = NeoSystem::new(
                ProtocolSettings::default(),
                Some(std::sync::Arc::from(provider)),
                Some(path.clone()),
            )
            .expect("create system with RocksDB");
            let hash = system.genesis_block().hash();
            assert_eq!(system.current_block_index(), 0);
            let system_weak = std::sync::Arc::downgrade(&system);
            let context_weak = std::sync::Arc::downgrade(&system.context());
            let store_weak = std::sync::Arc::downgrade(&system.store());
            system.shutdown().await.expect("shut down first system");
            assert_eq!(system_weak.strong_count(), 0, "system must be released");
            assert_eq!(context_weak.strong_count(), 0, "context must be released");
            assert_eq!(store_weak.strong_count(), 0, "RocksDB must be released");
            hash
        };

        // Inspect the reopened database before constructing another system so
        // this assertion cannot be satisfied by a hidden in-memory substitute.
        {
            let provider = select_store_provider(
                Some("rocksdb"),
                test_storage_config(dir.path().to_path_buf()),
            )
            .expect("reselect RocksDB")
            .expect("concrete RocksDB provider");
            let store = provider.get_store(&path).expect("reopen RocksDB directly");
            let cache = StoreCache::new_from_snapshot(store.snapshot());
            assert!(LedgerContract::new().contains_block(&cache, &expected_genesis_hash));
        }

        let provider = select_store_provider(
            Some("rocksdb"),
            test_storage_config(dir.path().to_path_buf()),
        )
        .expect("reselect RocksDB for system")
        .expect("concrete RocksDB provider");
        let system = NeoSystem::new(
            ProtocolSettings::default(),
            Some(std::sync::Arc::from(provider)),
            Some(path),
        )
        .expect("reopen system with persisted RocksDB state");
        assert_eq!(system.block_hash_at(0), Some(expected_genesis_hash));
        assert_eq!(system.current_block_index(), 0);
        system.shutdown().await.expect("shut down reopened system");
    }

    #[cfg(not(feature = "full"))]
    #[test]
    fn rocksdb_backend_fails_closed_without_full_feature() {
        let err = match select_store_provider(
            Some("rocksdb"),
            test_storage_config(PathBuf::from("./data")),
        ) {
            Err(err) => err,
            // A silent Ok would reintroduce the production bug this function
            // exists to fix: a node configured for persistent storage quietly
            // running on an ephemeral store.
            Ok(_) => panic!("rocksdb without the full feature must fail closed"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("full"),
            "error must explain the missing feature: {message}"
        );
        assert!(
            message.contains("memory"),
            "error must name the fallback alternative: {message}"
        );
    }

    #[test]
    fn leveldb_backend_fails_closed() {
        // No LevelDB StoreProvider implementation exists in neo-storage; the
        // option must be rejected instead of silently degrading to memory.
        let err = match select_store_provider(
            Some("leveldb"),
            test_storage_config(PathBuf::from("./data")),
        ) {
            Err(err) => err,
            Ok(_) => panic!("leveldb backend must fail closed"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("leveldb"),
            "error must name the rejected backend: {message}"
        );
    }

    #[test]
    fn unknown_backend_fails_closed() {
        let err = match select_store_provider(
            Some("sqlite"),
            test_storage_config(PathBuf::from("./data")),
        ) {
            Err(err) => err,
            Ok(_) => panic!("unknown backend must fail closed"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("sqlite"),
            "error must name the unknown backend: {message}"
        );
        assert!(
            message.contains("memory") && message.contains("rocksdb"),
            "error must list the supported backends: {message}"
        );
    }
}

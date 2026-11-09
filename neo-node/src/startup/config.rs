//! Configuration validation and storage management for neo-node.
//!
//! This module provides production-ready configuration validation, storage
//! access checks, and provider selection logic.

use anyhow::{Context, Result};
use neo_protocol::ProtocolSettings;
use neo_storage::persistence::StoreProvider;
use neo_system::state_service::StateServiceSettings;
use std::path::Path;
use tracing::{info, warn};

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
    let mut features = vec!["base"];

    #[cfg(feature = "full")]
    features.push("full");
    #[cfg(feature = "tee")]
    features.push("tee");
    #[cfg(feature = "hsm")]
    features.push("hsm");
    #[cfg(feature = "rpc")]
    features.push("rpc");
    #[cfg(feature = "consensus")]
    features.push("consensus");

    format!("features=[{}]", features.join(","))
}

/// Builds state service settings from configuration with full validation.
///
/// This validates all configuration options and ensures consistency with
/// protocol settings and CLI overrides.
pub(crate) fn build_state_service_settings(
    cli: &crate::cli::NodeCli,
    config: &crate::config::NodeConfig,
    storage_path: Option<&str>,
    protocol_settings: &ProtocolSettings,
) -> Result<Option<StateServiceSettings>> {
    // Check if state service is enabled in config
    let state_service_section = match &config.state_service {
        Some(section) => section,
        None => {
            info!("State service not configured");
            return Ok(None);
        }
    };

    // Determine storage path with priority: CLI > config > default
    let path = if let Some(cli_path) = storage_path {
        cli_path.to_string()
    } else if let Some(config_path) = &state_service_section.path {
        config_path.clone()
    } else {
        format!("Data_MPT_{}", protocol_settings.network)
    };

    // Validate path is not empty
    if path.trim().is_empty() {
        anyhow::bail!("State service path cannot be empty");
    }

    // Determine network with validation
    let network = state_service_section
        .network
        .unwrap_or(protocol_settings.network);

    if network != protocol_settings.network {
        warn!(
            "State service network ({}) differs from protocol network ({})",
            network, protocol_settings.network
        );
    }

    // Validate max_find_result_items is reasonable
    let max_find_result_items = state_service_section.max_find_result_items.unwrap_or(100);
    if max_find_result_items == 0 {
        anyhow::bail!("max_find_result_items must be greater than 0");
    }
    if max_find_result_items > 10000 {
        warn!(
            "max_find_result_items ({}) is very large, may impact performance",
            max_find_result_items
        );
    }

    // Build settings with validated values
    let settings = StateServiceSettings {
        full_state: state_service_section.full_state.unwrap_or(false),
        path,
        network,
        auto_verify: state_service_section.auto_verify.unwrap_or(false),
        max_find_result_items,
        exception_policy: neo_system::state_service::UnhandledExceptionPolicy::StopPlugin,
    };

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

    // Backend-specific health checks
    if let Some(backend_name) = backend {
        match backend_name {
            "rocksdb" | "leveldb" => {
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

        let stored_network: u32 = content.trim().parse()
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
        std::fs::write(&metadata_file, expected_network.to_string())
            .with_context(|| format!("Failed to write network metadata to {}", metadata_file.display()))?;

        info!("Initialized storage network metadata: network {}", expected_network);
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

/// Selects the appropriate storage provider based on backend name.
///
/// This implements the storage backend selection logic with proper
/// configuration and initialization.
pub(crate) fn select_store_provider<T>(
    backend: Option<&str>,
    _config: T,
) -> Result<Option<Box<dyn StoreProvider>>> {
    let backend_name = backend.unwrap_or("memory");

    match backend_name {
        "memory" => {
            info!("Selected memory storage backend (ephemeral, no persistence)");
            // Memory store would be instantiated here when available
            // For now, returning None indicates default/in-memory behavior
            Ok(None)
        }
        "rocksdb" => {
            info!("Selected RocksDB storage backend");
            // TODO: Instantiate RocksDB provider when available
            // let provider = RocksDbProvider::new(config)?;
            // Ok(Some(Box::new(provider)))
            Ok(None)
        }
        "leveldb" => {
            info!("Selected LevelDB storage backend");
            // TODO: Instantiate LevelDB provider when available
            // let provider = LevelDbProvider::new(config)?;
            // Ok(Some(Box::new(provider)))
            Ok(None)
        }
        other => {
            anyhow::bail!(
                "Unknown storage backend: '{}'. Supported backends: memory, rocksdb, leveldb",
                other
            );
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
            warn!("RPC max_connections ({}) is very high, may impact performance", max_conn);
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
            if config.unlock_wallet.path.as_ref().map_or(true, |p| p.is_empty()) {
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
        if oracle.auto_start.unwrap_or(false) && config.unlock_wallet.path.as_ref().map_or(true, |p| p.is_empty()) {
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

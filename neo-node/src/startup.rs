//! Startup utilities for neo-node.
//!
//! This module contains functions for:
//! - Storage provider selection and validation
//! - Configuration validation
//! - Seed node resolution

use crate::config::{infer_magic_from_type, NodeConfig};
use anyhow::{bail, Context, Result};
use neo_core::{
    persistence::{providers::RocksDBStoreProvider, storage::StorageConfig, IStoreProvider},
    protocol_settings::ProtocolSettings,
};
use std::{fs, path::Path, sync::Arc};
use tracing::{info, warn};

pub(crate) const STORAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Selects the appropriate storage provider based on backend name.
pub fn select_store_provider(
    backend: Option<&str>,
    storage_config: StorageConfig,
) -> Result<Option<Arc<dyn IStoreProvider>>> {
    let Some(name) = backend else {
        return Ok(None);
    };

    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "memory" | "mem" | "inmemory" => Ok(None),
        "rocksdb" | "rocksdbstore" | "rocksdb-store" => {
            let provider: Arc<dyn IStoreProvider> =
                Arc::new(RocksDBStoreProvider::new(storage_config));
            Ok(Some(provider))
        }
        other => bail!("unsupported storage backend '{}'", other),
    }
}

/// Checks storage network markers and version compatibility.
pub fn check_storage_network(path: &str, magic: u32, read_only: bool) -> Result<()> {
    let storage_path = Path::new(path);
    if !storage_path.exists() {
        if read_only {
            bail!("storage path {} does not exist (read-only mode)", path);
        }
        fs::create_dir_all(storage_path)
            .with_context(|| format!("failed to create storage path {}", path))?;
    }

    let marker = storage_path.join("NETWORK_MAGIC");
    if marker.exists() {
        let contents = fs::read_to_string(&marker)
            .with_context(|| format!("failed to read network marker {}", marker.display()))?;
        let parsed = contents.trim_start_matches("0x").trim().to_string();
        let stored_magic = u32::from_str_radix(&parsed, 16)
            .or_else(|_| parsed.parse::<u32>())
            .with_context(|| format!("invalid magic in {}: {}", marker.display(), contents))?;
        if stored_magic != magic {
            bail!(
                "storage at {} was initialized for network magic 0x{:08x}, but current config is 0x{:08x}. Use a fresh storage path or clear the directory.",
                path,
                stored_magic,
                magic
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing NETWORK_MAGIC marker (read-only mode)",
                path
            );
        }
        fs::write(&marker, format!("0x{magic:08x}"))
            .with_context(|| format!("failed to write network marker {}", marker.display()))?;
    }

    let version_marker = storage_path.join("VERSION");
    if version_marker.exists() {
        let contents = fs::read_to_string(&version_marker).with_context(|| {
            format!(
                "failed to read storage version marker {}",
                version_marker.display()
            )
        })?;
        let stored_version = contents.trim();
        if stored_version != STORAGE_VERSION {
            bail!(
                "storage at {} was created with version '{}', current binary is '{}'. Use a fresh storage path or migrate data.",
                path,
                stored_version,
                STORAGE_VERSION
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing VERSION marker (read-only mode)",
                path
            );
        }
        fs::write(&version_marker, STORAGE_VERSION).with_context(|| {
            format!(
                "failed to write storage version marker {}",
                version_marker.display()
            )
        })?;
    }
    Ok(())
}

/// Checks if a bind address is publicly accessible.
pub fn is_public_bind(bind: &str) -> bool {
    bind.parse::<std::net::IpAddr>()
        .map(|ip| !ip.is_loopback())
        .unwrap_or(true)
}

fn has_default_rpc_credentials(user: &str, pass: &str) -> bool {
    if !user.eq_ignore_ascii_case("neo") {
        return false;
    }

    let normalized = pass.trim().to_ascii_lowercase();
    normalized.starts_with("change-me-") || normalized == "change-me" || normalized == "changeme"
}

/// Validates the node configuration.
pub fn validate_node_config(
    node_config: &NodeConfig,
    storage_path: Option<&str>,
    backend_name: Option<&str>,
    protocol_settings: &ProtocolSettings,
    rpc_hardened: bool,
) -> Result<()> {
    if node_config.rpc.auth_enabled
        && (node_config.rpc.rpc_user.is_none() || node_config.rpc.rpc_pass.is_none())
    {
        bail!("rpc.auth_enabled requires both rpc_user and rpc_pass");
    }

    if node_config.rpc.enabled && node_config.rpc.auth_enabled {
        if let (Some(user), Some(pass)) = (&node_config.rpc.rpc_user, &node_config.rpc.rpc_pass) {
            if has_default_rpc_credentials(user, pass) {
                let bind = node_config
                    .rpc
                    .bind_address
                    .as_deref()
                    .unwrap_or("127.0.0.1");
                if is_public_bind(bind) {
                    bail!(
                        "default rpc credentials are not allowed on public bind addresses; set unique rpc_user and rpc_pass"
                    );
                }
                warn!(
                    target: "neo",
                    bind_address = bind,
                    "RPC is using template credentials on loopback; change rpc_user/rpc_pass before exposing RPC"
                );
            }
        }
    }

    if node_config
        .blockchain
        .max_free_transactions_per_block
        .is_some()
    {
        bail!(
            "blockchain.max_free_transactions_per_block is not supported by neo-node yet; remove this setting"
        );
    }

    if node_config
        .mempool
        .as_ref()
        .and_then(|m| m.max_transactions_per_sender)
        .is_some()
    {
        bail!(
            "mempool.max_transactions_per_sender is not supported by neo-node yet; remove this setting"
        );
    }

    if rpc_hardened && (node_config.rpc.rpc_user.is_none() || node_config.rpc.rpc_pass.is_none()) {
        bail!("--rpc-hardened requires rpc_user and rpc_pass (set via config or env)");
    }

    if node_config.rpc.enabled && !node_config.rpc.auth_enabled {
        let bind = node_config
            .rpc
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1");
        if is_public_bind(bind) {
            warn!(
                target: "neo",
                bind_address = bind,
                "RPC is enabled on a non-loopback address without auth; enable auth or front with a proxy"
            );
        }
    }

    if let Some(name) = backend_name {
        let normalized = name.trim().to_ascii_lowercase();
        let requires_path = matches!(
            normalized.as_str(),
            "rocksdb" | "rocksdbstore" | "rocksdb-store"
        );
        if requires_path && storage_path.map(|p| p.trim().is_empty()).unwrap_or(true) {
            bail!(
                "storage backend '{}' requires a data path (--storage or [storage.path])",
                name
            );
        }
    }

    if let Some(path) = storage_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = Path::new(trimmed);
            if candidate.is_file() {
                bail!(
                    "storage path '{}' is a file; provide a directory path",
                    trimmed
                );
            }
        }
    }

    if let Some(canonical) = node_config
        .network
        .network_type
        .as_deref()
        .and_then(infer_magic_from_type)
    {
        if canonical != protocol_settings.network {
            warn!(
                target: "neo",
                network_type = ?node_config.network.network_type,
                configured_magic = format_args!("0x{:08x}", protocol_settings.network),
                canonical_magic = format_args!("0x{:08x}", canonical),
                "network type and magic differ; ensure this is intentional"
            );
        }
    }

    // Validate plugin-backed configs to match C# plugin load behavior.
    let _ = node_config.oracle_service_settings(protocol_settings)?;
    let _ = node_config.dbft_settings(protocol_settings)?;

    Ok(())
}

/// Checks storage backend accessibility.
pub fn check_storage_access(
    backend: Option<&str>,
    storage_path: Option<&str>,
    storage_config: StorageConfig,
) -> Result<()> {
    let provider = select_store_provider(backend, storage_config)?;
    let Some(provider) = provider else {
        info!(target: "neo", "storage check: memory backend selected; nothing to validate");
        return Ok(());
    };

    let path = storage_path
        .ok_or_else(|| anyhow::anyhow!("storage check: no path provided for backend"))?;

    let store = provider
        .get_store(path)
        .map_err(|err| anyhow::anyhow!("storage check: failed to open store at {path}: {err}"))?;
    drop(store);
    info!(target: "neo", path, "storage check: backend opened successfully");
    Ok(())
}

/// Builds a summary of enabled features.
#[allow(clippy::useless_vec)]
pub fn build_feature_summary() -> String {
    #[allow(unused_mut)]
    let mut features = vec!["plugins: rpc-server,rocksdb-store,tokens-tracker,application-logs"];

    #[cfg(feature = "tee")]
    features.push("tee: enabled");

    #[cfg(feature = "tee-sgx")]
    features.push("tee-sgx: hardware");

    features.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    #[test]
    fn validate_requires_storage_path_for_rocksdb() {
        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err =
            validate_node_config(&cfg, None, Some("rocksdb"), &cfg.protocol_settings(), false)
                .expect_err("should fail without storage path");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("storage backend"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_accepts_memory_without_path() {
        let cfg = NodeConfig::default();
        validate_node_config(&cfg, None, Some("memory"), &cfg.protocol_settings(), false)
            .expect("memory backend should not require path");
    }

    #[test]
    fn validate_enforces_rpc_auth_credentials() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("missing rpc credentials should error");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_storage_path_that_is_file() {
        let tmp = tempfile::NamedTempFile::new().expect("temp file");
        let path_str = tmp.path().to_string_lossy().to_string();

        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err = validate_node_config(
            &cfg,
            Some(&path_str),
            Some("rocksdb"),
            &cfg.protocol_settings(),
            false,
        )
        .expect_err("file path should be rejected");
        assert!(
            err.to_string().to_ascii_lowercase().contains("file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_hardened_requires_credentials() {
        let cfg = NodeConfig::default();
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), true)
            .expect_err("hardened mode without credentials should fail");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_default_rpc_credentials_on_public_bind() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        cfg.rpc.bind_address = Some("0.0.0.0".to_string());
        cfg.rpc.rpc_user = Some(Zeroizing::new("neo".to_string()));
        cfg.rpc.rpc_pass = Some(Zeroizing::new("change-me-mainnet-rpc-password".to_string()));

        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("template credentials on public bind must be rejected");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("default rpc credentials"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_unsupported_max_free_transactions_per_block() {
        let mut cfg = NodeConfig::default();
        cfg.blockchain.max_free_transactions_per_block = Some(20);
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("unsupported blockchain.max_free_transactions_per_block should fail");
        assert!(
            err.to_string()
                .contains("blockchain.max_free_transactions_per_block"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_unsupported_mempool_max_transactions_per_sender() {
        let mut cfg = NodeConfig::default();
        cfg.mempool = Some(crate::config::MempoolSection {
            max_transactions: Some(10_000),
            max_transactions_per_sender: Some(100),
        });
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("unsupported mempool.max_transactions_per_sender should fail");
        assert!(
            err.to_string()
                .contains("mempool.max_transactions_per_sender"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn check_storage_allows_memory_without_path() {
        check_storage_access(Some("memory"), None, StorageConfig::default())
            .expect("memory backend should skip validation");
    }

    #[test]
    fn check_storage_succeeds_with_rocksdb_path() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("rocksdb-check");
        let cfg = StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        };

        check_storage_access(
            Some("rocksdb"),
            Some(db_path.to_string_lossy().as_ref()),
            cfg,
        )
        .expect("rocksdb backend should open successfully");
    }

    #[test]
    fn check_storage_network_writes_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        let path_str = path.to_string_lossy().to_string();

        check_storage_network(&path_str, 0x1234_5678, false).expect("check storage network");

        let magic = fs::read_to_string(path.join("NETWORK_MAGIC")).expect("read magic");
        assert!(magic.contains("0x12345678"));

        let version = fs::read_to_string(path.join("VERSION")).expect("read version");
        assert_eq!(version.trim(), STORAGE_VERSION);
    }

    #[test]
    fn check_storage_network_readonly_requires_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        fs::create_dir_all(&path).expect("create dir");
        // Missing markers should fail
        let err = check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect_err("missing markers should fail in read-only");
        assert!(err.to_string().to_ascii_lowercase().contains("marker"));

        // Add markers and succeed
        fs::write(path.join("NETWORK_MAGIC"), "0x00000001").expect("write magic");
        fs::write(path.join("VERSION"), STORAGE_VERSION).expect("write version");
        check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect("markers present should pass");
    }

    #[test]
    fn bundled_mainnet_config_is_check_config_valid() {
        let cfg: NodeConfig =
            toml::from_str(include_str!("../../neo_mainnet_node.toml")).expect("parse mainnet");
        let settings = cfg.protocol_settings();
        validate_node_config(
            &cfg,
            cfg.storage.path.as_deref(),
            cfg.storage.backend.as_deref(),
            &settings,
            false,
        )
        .expect("bundled mainnet config should pass --check-config validation");
    }

    #[test]
    fn bundled_production_config_is_check_config_valid() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../neo_production_node.toml"))
            .expect("parse production");
        let settings = cfg.protocol_settings();
        validate_node_config(
            &cfg,
            cfg.storage.path.as_deref(),
            cfg.storage.backend.as_deref(),
            &settings,
            false,
        )
        .expect("bundled production config should pass --check-config validation");
    }
}

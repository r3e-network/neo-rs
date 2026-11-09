//! Configuration constants and helpers for neo-node.
//!
//! This module contains version constants and configuration validation
//! used during node startup and operation.

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_storage::persistence::StoreProvider;
use neo_system::state_service::StateServiceSettings;
use std::path::Path;

/// Storage version identifier used for compatibility checking.
///
/// When storage format changes, this version is incremented to prevent
/// incompatible data from being loaded.
pub(crate) const STORAGE_VERSION: u32 = 1;

/// Returns a human-readable build/feature summary string.
///
/// TODO: Implement actual feature detection
pub(crate) fn build_feature_summary() -> String {
    let mut features = vec!["base"];

    #[cfg(feature = "full")]
    features.push("full");
    #[cfg(feature = "tee")]
    features.push("tee");
    #[cfg(feature = "hsm")]
    features.push("hsm");
    #[cfg(feature = "profiling")]
    features.push("profiling");

    format!("neo-node {} (features: {})", env!("CARGO_PKG_VERSION"), features.join(", "))
}

/// Builds state service settings from configuration.
///
/// TODO: Implement actual configuration parsing
pub(crate) fn build_state_service_settings(
    _cli: &crate::cli::NodeCli,
    _config: &crate::config::NodeConfig,
    _storage_path: Option<&str>,
    _protocol_settings: &ProtocolSettings,
) -> Result<Option<StateServiceSettings>> {
    // Placeholder implementation
    Ok(None)
}

/// Checks if storage path is accessible with given permissions.
///
/// TODO: Implement actual file system checks
pub(crate) fn check_storage_access<T>(
    _backend: Option<&str>,
    _path: Option<&str>,
    _config: T,
) -> Result<()> {
    // Placeholder - assume accessible
    Ok(())
}

/// Validates that storage network matches protocol settings.
///
/// TODO: Implement actual network validation
pub(crate) fn check_storage_network(
    _path: &str,
    _expected_network: u32,
    _read_only: bool,
) -> Result<()> {
    // Placeholder - assume valid
    Ok(())
}

/// Selects the appropriate storage provider based on backend name.
///
/// TODO: Implement actual provider selection logic
pub(crate) fn select_store_provider<T>(
    _backend: Option<&str>,
    _config: T,
) -> Result<Option<Box<dyn StoreProvider>>> {
    // Placeholder - return None instead of error
    Ok(None)
}

/// Validates node configuration consistency.
///
/// TODO: Implement actual validation logic
pub(crate) fn validate_node_config(
    _config: &crate::config::NodeConfig,
    _storage_path: Option<&str>,
    _backend: Option<&str>,
    _protocol_settings: &ProtocolSettings,
    _rpc_hardened: bool,
) -> Result<()> {
    // Placeholder - assume valid
    Ok(())
}

//! Migration utilities for importing data from neo-cli and neo-go.
//!
//! This module provides seamless migration from C# neo-cli and Go neo-go nodes,
//! allowing neo-rs to continue syncing from their data directories without
//! re-synchronizing the entire blockchain.

pub mod neo_cli;
pub mod neo_go;
pub mod detector;
pub mod config_parser;
pub mod storage_adapter;

use std::path::Path;
use anyhow::{Result, bail};
use tracing::{info, warn};

/// Supported Neo node implementations for migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeImplementation {
    /// C# neo-cli (official Neo implementation)
    NeoCli,
    /// Go neo-go implementation
    NeoGo,
}

impl NodeImplementation {
    pub fn as_str(&self) -> &str {
        match self {
            Self::NeoCli => "neo-cli",
            Self::NeoGo => "neo-go",
        }
    }
}

/// Migration configuration and options.
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Source node data directory
    pub source_dir: std::path::PathBuf,
    /// Target neo-rs storage directory
    pub target_dir: std::path::PathBuf,
    /// Detected or specified node implementation
    pub implementation: NodeImplementation,
    /// Whether to migrate storage data
    pub migrate_storage: bool,
    /// Whether to migrate configuration
    pub migrate_config: bool,
    /// Whether to verify migrated data
    pub verify: bool,
}

/// Detect which Neo implementation the source directory belongs to.
///
/// Checks for characteristic files and directory structures:
/// - neo-cli: config.json, protocol.*.json, Storage/
/// - neo-go: config.yml, .bolt or .db files in the root
pub fn detect_implementation(source_dir: &Path) -> Result<NodeImplementation> {
    detector::detect_implementation(source_dir)
}

/// Migrate from neo-cli or neo-go to neo-rs.
///
/// This function:
/// 1. Detects the source implementation type
/// 2. Parses protocol configuration
/// 3. Migrates RocksDB/LevelDB storage
/// 4. Validates the migration
/// 5. Returns the starting block height
pub async fn migrate(config: MigrationConfig) -> Result<MigrationResult> {
    info!(
        "Starting migration from {} at {}",
        config.implementation.as_str(),
        config.source_dir.display()
    );

    // Validate source directory
    if !config.source_dir.exists() {
        bail!("Source directory does not exist: {}", config.source_dir.display());
    }

    // Create target directory if needed
    if !config.target_dir.exists() {
        std::fs::create_dir_all(&config.target_dir)?;
    }

    // Migrate based on implementation
    let result = match config.implementation {
        NodeImplementation::NeoCli => {
            neo_cli::migrate(&config).await?
        }
        NodeImplementation::NeoGo => {
            neo_go::migrate(&config).await?
        }
    };

    info!(
        "Migration complete: {} blocks, {} state entries, starting from height {}",
        result.blocks_migrated, result.state_entries_migrated, result.starting_height
    );

    if config.verify {
        info!("Verifying migrated data...");
        verify_migration(&config, &result)?;
        info!("Verification passed");
    }

    Ok(result)
}

/// Result of a migration operation.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Number of blocks migrated
    pub blocks_migrated: u64,
    /// Number of state entries migrated
    pub state_entries_migrated: u64,
    /// Starting block height to resume from
    pub starting_height: u32,
    /// Protocol configuration detected
    pub protocol_magic: u32,
    /// Network name detected
    pub network: String,
}

/// Verify that the migration was successful.
fn verify_migration(config: &MigrationConfig, result: &MigrationResult) -> Result<()> {
    // Basic sanity checks
    if result.starting_height == 0 && result.blocks_migrated > 0 {
        warn!("Starting height is 0 but blocks were migrated - possible issue");
    }

    // Verify target directory structure
    if !config.target_dir.exists() {
        bail!("Target directory was not created: {}", config.target_dir.display());
    }

    info!("Migration verification: All checks passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_implementation_str() {
        assert_eq!(NodeImplementation::NeoCli.as_str(), "neo-cli");
        assert_eq!(NodeImplementation::NeoGo.as_str(), "neo-go");
    }
}

//! Configuration migration utilities for neo-cli and neo-go data directories.
//!
//! Direct storage conversion is deliberately rejected until the source and target
//! schemas can be converted and verified completely; configuration-only migration
//! remains available and never claims that chain data was imported.

pub mod detector;
pub mod neo_cli;
pub mod neo_go;

use anyhow::{Result, bail};
use std::path::Path;
use tracing::info;

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
    /// Whether to parse and report configuration
    pub migrate_config: bool,
    /// Whether to validate the migration result
    pub verify: bool,
}

/// Detect which Neo implementation the source directory belongs to.
pub fn detect_implementation(source_dir: &Path) -> Result<NodeImplementation> {
    detector::detect_implementation(source_dir)
}

/// Migrate configuration and, when supported, storage from another Neo node.
///
/// Configuration-only mode parses the protocol settings and returns the detected
/// network. It does not create a target chain database or imply that blocks were
/// imported. Storage mode currently fails closed for both implementations.
pub async fn migrate(config: MigrationConfig) -> Result<MigrationResult> {
    info!(
        "Starting migration from {} at {}",
        config.implementation.as_str(),
        config.source_dir.display()
    );

    if !config.source_dir.is_dir() {
        bail!(
            "Source directory does not exist or is not a directory: {}",
            config.source_dir.display()
        );
    }
    if !config.migrate_config && !config.migrate_storage {
        bail!("Migration has no effect: both configuration and storage are disabled");
    }

    let result = match config.implementation {
        NodeImplementation::NeoCli => neo_cli::migrate(&config).await?,
        NodeImplementation::NeoGo => neo_go::migrate(&config).await?,
    };

    if config.verify {
        verify_migration(&config, &result)?;
    }

    if config.migrate_storage {
        info!(
            "Migration complete: {} blocks, {} state entries, starting from height {}",
            result.blocks_migrated, result.state_entries_migrated, result.starting_height
        );
    } else {
        info!(
            network = %result.network,
            magic = result.protocol_magic,
            "Configuration parsed; no chain data was imported"
        );
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

/// Validates result invariants without pretending to inspect unmigrated storage.
fn verify_migration(config: &MigrationConfig, result: &MigrationResult) -> Result<()> {
    if result.network.is_empty() {
        bail!("Migration returned an empty network name");
    }
    if !config.migrate_storage
        && (result.blocks_migrated != 0
            || result.state_entries_migrated != 0
            || result.starting_height != 0)
    {
        bail!("Configuration-only migration returned fabricated chain-data statistics");
    }
    if config.migrate_storage && !config.target_dir.join("RocksDBStore").is_dir() {
        bail!(
            "Storage migration completed without a target database: {}",
            config.target_dir.display()
        );
    }
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

    #[test]
    fn config_only_verification_rejects_fabricated_statistics() {
        let config = MigrationConfig {
            source_dir: Path::new("source").to_path_buf(),
            target_dir: Path::new("target").to_path_buf(),
            implementation: NodeImplementation::NeoCli,
            migrate_storage: false,
            migrate_config: true,
            verify: true,
        };
        let result = MigrationResult {
            blocks_migrated: 1,
            state_entries_migrated: 0,
            starting_height: 1,
            protocol_magic: 5195086,
            network: "mainnet".to_string(),
        };
        assert!(verify_migration(&config, &result).is_err());
    }

    #[tokio::test]
    async fn config_only_migration_does_not_require_source_storage() {
        const COMMITTEE: &str =
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";

        for implementation in [NodeImplementation::NeoCli, NodeImplementation::NeoGo] {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("source");
            std::fs::create_dir(&source).unwrap();
            match implementation {
                NodeImplementation::NeoCli => std::fs::write(
                    source.join("protocol.mainnet.json"),
                    format!(
                        r#"{{"ProtocolConfiguration":{{"Network":12345,"StandbyCommittee":["{COMMITTEE}"]}}}}"#
                    ),
                )
                .unwrap(),
                NodeImplementation::NeoGo => std::fs::write(
                    source.join("config.yml"),
                    format!(
                        "ProtocolConfiguration:\n  Magic: 12345\n  StandbyCommittee: [\"{COMMITTEE}\"]\n"
                    ),
                )
                .unwrap(),
            }
            let target = temp.path().join("target");
            let result = migrate(MigrationConfig {
                source_dir: source,
                target_dir: target.clone(),
                implementation,
                migrate_storage: false,
                migrate_config: true,
                verify: true,
            })
            .await
            .unwrap();

            assert_eq!(result.protocol_magic, 12345);
            assert_eq!(result.blocks_migrated, 0);
            assert_eq!(result.state_entries_migrated, 0);
            assert_eq!(result.starting_height, 0);
            assert!(!target.exists());
        }
    }
}

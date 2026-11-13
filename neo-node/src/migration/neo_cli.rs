//! Migration from C# neo-cli to neo-rs.

use super::{MigrationConfig, MigrationResult};
use anyhow::{Context, Result, bail};
use neo_protocol::ProtocolSettings;
use std::path::Path;
use tracing::{debug, info};

/// Migrate from neo-cli to neo-rs.
///
/// neo-cli storage structure:
/// - Storage/LevelDBStore/ - older versions use LevelDB
/// - Storage/RocksDBStore/ - newer versions use RocksDB
/// - config.json - main configuration
/// - protocol.mainnet.json / protocol.testnet.json - protocol settings
pub async fn migrate(config: &MigrationConfig) -> Result<MigrationResult> {
    info!("Migrating from neo-cli at {}", config.source_dir.display());

    // Step 1: Parse protocol configuration
    let protocol_settings = parse_protocol_config(&config.source_dir)?;
    info!("Detected network: magic={}", protocol_settings.network);

    // Configuration-only migration must not require a source chain database.
    let (blocks_migrated, state_entries, starting_height) = if config.migrate_storage {
        let storage_path = detect_storage_path(&config.source_dir)?;
        info!("Found storage at: {}", storage_path.display());
        migrate_storage(&storage_path, &config.target_dir, &protocol_settings).await?
    } else {
        info!("Skipping storage detection and migration (config-only mode)");
        (0, 0, 0)
    };

    Ok(MigrationResult {
        blocks_migrated,
        state_entries_migrated: state_entries,
        starting_height,
        protocol_magic: protocol_settings.network,
        network: detect_network_name(&protocol_settings),
    })
}

/// Parse protocol configuration from neo-cli directory.
///
/// Looks for protocol.*.json files and parses them.
fn parse_protocol_config(source_dir: &Path) -> Result<ProtocolSettings> {
    // Look for protocol.*.json files
    let entries = std::fs::read_dir(source_dir)?;

    for entry in entries.filter_map(std::result::Result::ok) {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str.starts_with("protocol.") && name_str.ends_with(".json") {
            debug!("Parsing protocol config: {}", name_str);
            let content = std::fs::read_to_string(entry.path())?;
            let settings = parse_protocol_json(&content)?;
            return Ok(settings);
        }
    }

    bail!("No protocol.*.json file found in {}", source_dir.display())
}

/// Parse protocol JSON into ProtocolSettings.
fn parse_protocol_json(json: &str) -> Result<ProtocolSettings> {
    let value: serde_json::Value =
        serde_json::from_str(json).context("Failed to parse protocol JSON")?;
    let protocol = value
        .get("ProtocolConfiguration")
        .context("Missing ProtocolConfiguration")?;
    if protocol.get("Network").is_none() {
        bail!("Missing Network magic");
    }
    if protocol.get("StandbyCommittee").is_none() {
        bail!("Missing StandbyCommittee");
    }

    // Delegate every consensus-critical field, including hardfork heights,
    // validator count, gas distribution and trace limits, to the canonical loader.
    ProtocolSettings::load_from_stream(&mut json.as_bytes()).map_err(anyhow::Error::msg)
}

/// Detect storage directory (RocksDB or LevelDB).
fn detect_storage_path(source_dir: &Path) -> Result<std::path::PathBuf> {
    let storage_base = source_dir.join("Storage");

    // Try RocksDB first (newer neo-cli versions)
    let rocksdb = storage_base.join("RocksDBStore");
    if rocksdb.exists() {
        return Ok(rocksdb);
    }

    // Try LevelDB (older versions)
    let leveldb = storage_base.join("LevelDBStore");
    if leveldb.exists() {
        return Ok(leveldb);
    }

    bail!(
        "No RocksDBStore or LevelDBStore found in {}",
        storage_base.display()
    )
}

/// Rejects direct storage copying until schema conversion is implemented.
///
/// Historical neo-cli key prefixes differ from the ledger-prefixed neo-rs layout,
/// and LevelDB files are not RocksDB files. Configuration-only migration remains
/// available through [`migrate`].
async fn migrate_storage(
    source_storage: &Path,
    target_dir: &Path,
    _protocol_settings: &ProtocolSettings,
) -> Result<(u64, u64, u32)> {
    // neo-rs uses ledger-prefixed keys (including the signed ledger id), not the
    // historical one-byte neo-cli layout previously assumed here. LevelDB also
    // cannot be opened as RocksDB. Refuse every direct copy until conversion and
    // completeness validation cover the actual source and target schemas.
    bail!(
        "neo-cli storage migration is not supported yet (source: {}, target: {}); \
         use an offline verified block export/import instead of copying LevelDB or RocksDB files",
        source_storage.display(),
        target_dir.display()
    )
}

/// Detect network name from protocol settings.
fn detect_network_name(settings: &ProtocolSettings) -> String {
    if settings.network == ProtocolSettings::mainnet().network {
        "mainnet".to_string()
    } else if settings.network == ProtocolSettings::testnet().network {
        "testnet".to_string()
    } else {
        format!("custom-{}", settings.network)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_protocol_json() {
        let json = r#"{
            "ProtocolConfiguration": {
                "Network": 5195086,
                "AddressVersion": 53,
                "StandbyCommittee": [
                    "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"
                ],
                "ValidatorsCount": 1,
                "SeedList": ["seed1.neo.org:10333"],
                "MaxTransactionsPerBlock": 512,
                "MemoryPoolMaxTransactions": 50000,
                "MillisecondsPerBlock": 15000,
                "Hardforks": {
                    "HF_Aspidochelone": 0,
                    "HF_Basilisk": 12
                }
            }
        }"#;

        let settings = parse_protocol_json(json).unwrap();
        assert_eq!(settings.network, 5195086);
        assert_eq!(settings.address_version, 53);
        assert_eq!(settings.validators_count, 1);
        assert_eq!(
            settings.hardforks.get(&neo_protocol::Hardfork::HfBasilisk),
            Some(&12)
        );
    }

    #[test]
    fn test_detect_network_name() {
        let mut settings = ProtocolSettings::default();
        settings.network = ProtocolSettings::mainnet().network;
        assert_eq!(detect_network_name(&settings), "mainnet");

        settings.network = ProtocolSettings::testnet().network;
        assert_eq!(detect_network_name(&settings), "testnet");

        settings.network = 12345;
        assert_eq!(detect_network_name(&settings), "custom-12345");
    }

    #[tokio::test]
    async fn unsupported_storage_copy_fails_without_creating_target_storage() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("LevelDBStore");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("CURRENT"), b"leveldb metadata").unwrap();
        let target = temp.path().join("target");

        let error = migrate_storage(&source, &target, &ProtocolSettings::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not supported"));
        assert!(!target.join("RocksDBStore").exists());
    }
}

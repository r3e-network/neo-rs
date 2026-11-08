//! Migration from Go neo-go to neo-rs.

use super::{MigrationConfig, MigrationResult};
use anyhow::{Result, Context, bail};
use neo_protocol::ProtocolSettings;
use std::path::Path;
use tracing::{info, debug};

/// Migrate from neo-go to neo-rs.
///
/// neo-go storage structure:
/// - chains/mainnet/*.db - BoltDB storage
/// - config.yml - configuration file
/// - Different key encoding than neo-cli
pub async fn migrate(config: &MigrationConfig) -> Result<MigrationResult> {
    info!("Migrating from neo-go at {}", config.source_dir.display());

    // Step 1: Parse protocol configuration from config.yml
    let protocol_settings = parse_config_yml(&config.source_dir)?;
    info!("Detected network: magic={}", protocol_settings.magic);

    // Step 2: Detect BoltDB storage
    let db_path = detect_bolt_db(&config.source_dir)?;
    info!("Found BoltDB at: {}", db_path.display());

    // Step 3: Convert storage if requested
    let (blocks_migrated, state_entries, starting_height) = if config.migrate_storage {
        convert_bolt_to_rocksdb(&db_path, &config.target_dir, &protocol_settings).await?
    } else {
        info!("Skipping storage migration (config-only mode)");
        (0, 0, 0)
    };

    Ok(MigrationResult {
        blocks_migrated,
        state_entries_migrated: state_entries,
        starting_height,
        protocol_magic: protocol_settings.magic,
        network: detect_network_name(&protocol_settings),
    })
}

/// Parse neo-go config.yml
fn parse_config_yml(source_dir: &Path) -> Result<ProtocolSettings> {
    let config_path = if source_dir.join("config.yml").exists() {
        source_dir.join("config.yml")
    } else if source_dir.join("config.yaml").exists() {
        source_dir.join("config.yaml")
    } else {
        bail!("No config.yml or config.yaml found in {}", source_dir.display())
    };

    let content = std::fs::read_to_string(&config_path)?;
    parse_yaml_config(&content)
}

/// Parse YAML configuration into ProtocolSettings
fn parse_yaml_config(yaml: &str) -> Result<ProtocolSettings> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)
        .context("Failed to parse config.yml")?;

    // Extract key fields from neo-go config format
    let protocol_config = &value["ProtocolConfiguration"];

    let magic = protocol_config["Magic"]
        .as_u64()
        .context("Missing Magic in config")? as u32;

    let address_version = protocol_config["AddressVersion"]
        .as_u64()
        .unwrap_or(53) as u8;

    let standby_committee: Vec<String> = protocol_config["StandbyCommittee"]
        .as_sequence()
        .context("Missing StandbyCommittee")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let seed_list: Vec<String> = protocol_config["SeedList"]
        .as_sequence()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let max_transactions_per_block = protocol_config["MaxTransactionsPerBlock"]
        .as_u64()
        .unwrap_or(512) as u32;

    let memory_pool_max_transactions = protocol_config["MemoryPoolMaxTransactions"]
        .as_u64()
        .unwrap_or(50000) as u32;

    let milliseconds_per_block = protocol_config["TimePerBlock"]
        .as_str()
        .and_then(|s| s.trim_end_matches('s').parse::<u32>().ok())
        .map(|s| s * 1000)
        .unwrap_or(15000);

    Ok(ProtocolSettings::new(
        magic,
        address_version,
        standby_committee,
        seed_list,
        max_transactions_per_block,
        memory_pool_max_transactions,
        milliseconds_per_block,
    ))
}

/// Detect BoltDB file location
fn detect_bolt_db(source_dir: &Path) -> Result<std::path::PathBuf> {
    // neo-go typically stores in chains/<network>/
    let chains_dir = source_dir.join("chains");

    if chains_dir.exists() {
        for entry in std::fs::read_dir(&chains_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                // Look for .db or .bolt files
                for db_file in std::fs::read_dir(entry.path())? {
                    let db_file = db_file?;
                    let name = db_file.file_name();
                    let name_str = name.to_string_lossy();

                    if name_str.ends_with(".db") || name_str.ends_with(".bolt") {
                        return Ok(db_file.path());
                    }
                }
            }
        }
    }

    // Also check root directory for .db files
    for entry in std::fs::read_dir(source_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.ends_with(".db") || name_str.ends_with(".bolt") {
            return Ok(entry.path());
        }
    }

    bail!("No BoltDB file found in {}", source_dir.display())
}

/// Convert BoltDB to RocksDB format.
///
/// neo-go uses BoltDB with different key encoding:
/// - Block: DataBlock + hash
/// - Transaction: DataTransaction + hash
/// - State: different bucket structure
///
/// We need to convert to neo-rs/neo-cli RocksDB key format.
async fn convert_bolt_to_rocksdb(
    bolt_path: &Path,
    target_dir: &Path,
    _protocol_settings: &ProtocolSettings,
) -> Result<(u64, u64, u32)> {
    info!("Converting BoltDB to RocksDB...");
    info!("Source: {}", bolt_path.display());
    info!("Target: {}", target_dir.display());

    // This is the complex part: need to open BoltDB and convert each entry
    // to RocksDB format with neo-cli compatible keys

    use rocksdb::{DB, Options};

    let target_storage = target_dir.join("RocksDBStore");
    std::fs::create_dir_all(&target_storage)?;

    // Open target RocksDB
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_max_open_files(1000);

    let target_db = DB::open(&opts, &target_storage)
        .context("Failed to create target RocksDB")?;

    // Open source BoltDB and iterate
    let blocks_migrated = convert_bolt_data(bolt_path, &target_db)?;

    // Read final height
    let starting_height = read_bolt_height(bolt_path)?;

    info!("Converted {} blocks, starting height: {}", blocks_migrated, starting_height);

    Ok((blocks_migrated, 0, starting_height))
}

/// Convert BoltDB data to RocksDB
fn convert_bolt_data(_bolt_path: &Path, _target_db: &rocksdb::DB) -> Result<u64> {
    // Implementation would:
    // 1. Open BoltDB read-only
    // 2. Iterate through buckets
    // 3. Convert keys from neo-go format to neo-cli format
    // 4. Write to RocksDB

    // For now, return placeholder
    // Full implementation requires bolt crate and detailed key mapping

    info!("BoltDB conversion: This is a complex operation that requires bolt crate");
    info!("For production use, consider using neo-go's export and neo-rs import");

    Ok(0)
}

/// Read current height from BoltDB
fn read_bolt_height(_bolt_path: &Path) -> Result<u32> {
    // Read height from BoltDB metadata
    // For now, return 0 (would need bolt crate to implement)
    Ok(0)
}

/// Detect network name from protocol settings
fn detect_network_name(settings: &ProtocolSettings) -> String {
    match settings.magic {
        5195086 => "mainnet".to_string(),
        1951352142 => "testnet".to_string(),
        _ => format!("custom-{}", settings.magic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_network_name() {
        let mut settings = ProtocolSettings::default();
        settings.magic = 5195086;
        assert_eq!(detect_network_name(&settings), "mainnet");
    }
}

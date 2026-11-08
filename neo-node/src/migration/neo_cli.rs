//! Migration from C# neo-cli to neo-rs.

use super::{MigrationConfig, MigrationResult};
use anyhow::{Result, Context, bail};
use neo_storage::persistence::StoreCache;
use neo_protocol::ProtocolSettings;
use std::path::Path;
use std::sync::Arc;
use tracing::{info, debug};

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
    info!("Detected network: magic={}", protocol_settings.magic);

    // Step 2: Detect storage backend
    let storage_path = detect_storage_path(&config.source_dir)?;
    info!("Found storage at: {}", storage_path.display());

    // Step 3: Migrate storage if requested
    let (blocks_migrated, state_entries, starting_height) = if config.migrate_storage {
        migrate_storage(&storage_path, &config.target_dir, &protocol_settings).await?
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

/// Parse protocol configuration from neo-cli directory.
///
/// Looks for protocol.*.json files and parses them.
fn parse_protocol_config(source_dir: &Path) -> Result<ProtocolSettings> {
    // Look for protocol.*.json files
    let entries = std::fs::read_dir(source_dir)?;

    for entry in entries.filter_map(|e| e.ok()) {
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
    let value: serde_json::Value = serde_json::from_str(json)
        .context("Failed to parse protocol JSON")?;

    // Extract key fields from neo-cli protocol.json format
    let magic = value["ProtocolConfiguration"]["Network"]
        .as_u64()
        .context("Missing Network magic")? as u32;

    let address_version = value["ProtocolConfiguration"]["AddressVersion"]
        .as_u64()
        .unwrap_or(53) as u8;

    let standby_committee: Vec<String> = value["ProtocolConfiguration"]["StandbyCommittee"]
        .as_array()
        .context("Missing StandbyCommittee")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let seed_list: Vec<String> = value["ProtocolConfiguration"]["SeedList"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let max_transactions_per_block = value["ProtocolConfiguration"]["MaxTransactionsPerBlock"]
        .as_u64()
        .unwrap_or(512) as u32;

    let memory_pool_max_transactions = value["ProtocolConfiguration"]["MemoryPoolMaxTransactions"]
        .as_u64()
        .unwrap_or(50000) as u32;

    let milliseconds_per_block = value["ProtocolConfiguration"]["MillisecondsPerBlock"]
        .as_u64()
        .unwrap_or(15000) as u32;

    // Build ProtocolSettings
    // Note: This is a simplified conversion; full implementation would handle all fields
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

    bail!("No RocksDBStore or LevelDBStore found in {}", storage_base.display())
}

/// Migrate storage data from neo-cli to neo-rs.
///
/// neo-cli key encoding:
/// - Block: 0x01 + block_hash (32 bytes)
/// - Transaction: 0x02 + tx_hash (32 bytes)
/// - State: 0x40 + contract_id (varint) + key
///
/// neo-rs uses the same RocksDB format, so we can copy directly or symlink.
async fn migrate_storage(
    source_storage: &Path,
    target_dir: &Path,
    _protocol_settings: &ProtocolSettings,
) -> Result<(u64, u64, u32)> {
    info!("Migrating storage from {} to {}", source_storage.display(), target_dir.display());

    // Strategy 1: Direct copy (safest but slower)
    // Strategy 2: Symlink (fastest but requires same filesystem)
    // Strategy 3: RocksDB backup/restore (balanced)

    // For now, use direct copy
    let target_storage = target_dir.join("RocksDBStore");

    if target_storage.exists() {
        bail!("Target storage already exists: {}", target_storage.display());
    }

    info!("Copying RocksDB data...");
    copy_dir_all(source_storage, &target_storage)?;

    // Read metadata to get block height
    let height = read_current_height(&target_storage)?;

    info!("Storage migrated: current height = {}", height);

    // Count blocks and state entries (approximate)
    let blocks = height as u64;
    let state_entries = estimate_state_entries(&target_storage)?;

    Ok((blocks, state_entries, height))
}

/// Recursively copy a directory.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;

        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }

    Ok(())
}

/// Read current block height from storage.
fn read_current_height(storage_path: &Path) -> Result<u32> {
    // Open RocksDB and read the current block height
    // Key: 0x05 (SYS_CurrentBlock)

    use rocksdb::{DB, Options};

    let mut opts = Options::default();
    opts.set_max_open_files(1000);

    let db = DB::open_for_read_only(&opts, storage_path, false)
        .context("Failed to open RocksDB")?;

    let key = [0x05u8]; // SYS_CurrentBlock prefix

    if let Some(value) = db.get(&key)? {
        // Value format: block_hash (32 bytes) + height (4 bytes LE)
        if value.len() >= 36 {
            let height_bytes = &value[32..36];
            let height = u32::from_le_bytes([
                height_bytes[0],
                height_bytes[1],
                height_bytes[2],
                height_bytes[3],
            ]);
            return Ok(height);
        }
    }

    Ok(0)
}

/// Estimate number of state entries.
fn estimate_state_entries(_storage_path: &Path) -> Result<u64> {
    // Rough estimate: count keys with prefix 0x40 (ST_Storage)
    // For now, return a placeholder
    Ok(0)
}

/// Detect network name from protocol settings.
fn detect_network_name(settings: &ProtocolSettings) -> String {
    match settings.magic {
        5195086 => "mainnet".to_string(),      // 0x004F454E (Neo N3 MainNet)
        1951352142 => "testnet".to_string(),   // 0x7474654E (Neo N3 TestNet)
        _ => format!("custom-{}", settings.magic),
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
                "SeedList": ["seed1.neo.org:10333"],
                "MaxTransactionsPerBlock": 512,
                "MemoryPoolMaxTransactions": 50000,
                "MillisecondsPerBlock": 15000
            }
        }"#;

        let settings = parse_protocol_json(json).unwrap();
        assert_eq!(settings.magic, 5195086);
        assert_eq!(settings.address_version, 53);
    }

    #[test]
    fn test_detect_network_name() {
        let mut settings = ProtocolSettings::default();
        settings.magic = 5195086;
        assert_eq!(detect_network_name(&settings), "mainnet");

        settings.magic = 1951352142;
        assert_eq!(detect_network_name(&settings), "testnet");

        settings.magic = 12345;
        assert_eq!(detect_network_name(&settings), "custom-12345");
    }
}

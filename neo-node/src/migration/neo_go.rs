//! Migration from Go neo-go to neo-rs.

use super::{MigrationConfig, MigrationResult};
use anyhow::{Context, Result, bail};
use neo_protocol::ProtocolSettings;
use std::path::Path;
use tracing::info;

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
    info!("Detected network: magic={}", protocol_settings.network);

    // Configuration-only migration must not require a source chain database.
    let (blocks_migrated, state_entries, starting_height) = if config.migrate_storage {
        let db_path = detect_bolt_db(&config.source_dir)?;
        info!("Found BoltDB at: {}", db_path.display());
        convert_bolt_to_rocksdb(&db_path, &config.target_dir, &protocol_settings).await?
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

/// Parse neo-go config.yml
fn parse_config_yml(source_dir: &Path) -> Result<ProtocolSettings> {
    let config_path = if source_dir.join("config.yml").exists() {
        source_dir.join("config.yml")
    } else if source_dir.join("config.yaml").exists() {
        source_dir.join("config.yaml")
    } else {
        bail!(
            "No config.yml or config.yaml found in {}",
            source_dir.display()
        )
    };

    let content = std::fs::read_to_string(&config_path)?;
    parse_yaml_config(&content)
}

/// Parse YAML configuration into ProtocolSettings
fn parse_yaml_config(yaml: &str) -> Result<ProtocolSettings> {
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(yaml).context("Failed to parse config.yml")?;
    let mut value = serde_json::to_value(yaml_value).context("Failed to normalize config.yml")?;
    let protocol = value
        .get_mut("ProtocolConfiguration")
        .and_then(serde_json::Value::as_object_mut)
        .context("Missing ProtocolConfiguration")?;

    let magic = protocol
        .remove("Magic")
        .context("Missing Magic in config")?;
    protocol.insert("Network".to_string(), magic);
    if !protocol.contains_key("StandbyCommittee") {
        bail!("Missing StandbyCommittee");
    }
    if let Some(duration) = protocol.remove("TimePerBlock") {
        let milliseconds = parse_duration_milliseconds(&duration)?;
        protocol.insert("MillisecondsPerBlock".to_string(), milliseconds.into());
    }

    let json = serde_json::to_vec(&value).context("Failed to encode normalized configuration")?;
    ProtocolSettings::load_from_stream(&mut json.as_slice()).map_err(anyhow::Error::msg)
}

fn parse_duration_milliseconds(value: &serde_json::Value) -> Result<u64> {
    if let Some(milliseconds) = value.as_u64() {
        return Ok(milliseconds);
    }
    let duration = value.as_str().context("TimePerBlock must be a duration")?;
    if let Some(milliseconds) = duration.strip_suffix("ms") {
        return milliseconds
            .parse::<u64>()
            .context("Invalid TimePerBlock milliseconds");
    }
    if let Some(seconds) = duration.strip_suffix('s') {
        return seconds
            .parse::<u64>()
            .context("Invalid TimePerBlock seconds")?
            .checked_mul(1000)
            .context("TimePerBlock exceeds the supported range");
    }
    bail!("TimePerBlock must use 's' or 'ms' units")
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

/// Rejects BoltDB conversion until its bucket and key mapping is fully verified.
async fn convert_bolt_to_rocksdb(
    bolt_path: &Path,
    target_dir: &Path,
    _protocol_settings: &ProtocolSettings,
) -> Result<(u64, u64, u32)> {
    // Do not create a target database until the complete neo-go bucket/key mapping
    // is implemented and verified. Returning an empty successful migration would
    // make a node resume from an unrelated genesis state.
    bail!(
        "neo-go BoltDB storage migration is not supported yet (source: {}, target: {}); \
         export blocks with neo-go and import them into neo-rs instead",
        bolt_path.display(),
        target_dir.display()
    )
}

/// Detect network name from protocol settings
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
    fn test_detect_network_name() {
        let mut settings = ProtocolSettings::default();
        settings.network = ProtocolSettings::mainnet().network;
        assert_eq!(detect_network_name(&settings), "mainnet");

        settings.network = ProtocolSettings::testnet().network;
        assert_eq!(detect_network_name(&settings), "testnet");
    }

    #[test]
    fn yaml_migration_preserves_consensus_critical_settings() {
        let yaml = r#"
ProtocolConfiguration:
  Magic: 12345
  AddressVersion: 53
  StandbyCommittee:
    - 03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c
  ValidatorsCount: 1
  SeedList: ["seed1.example.org:10333"]
  TimePerBlock: 500ms
  Hardforks:
    HF_Aspidochelone: 0
    HF_Basilisk: 12
"#;
        let settings = parse_yaml_config(yaml).unwrap();
        assert_eq!(settings.network, 12345);
        assert_eq!(settings.validators_count, 1);
        assert_eq!(settings.milliseconds_per_block, 500);
        assert_eq!(
            settings.hardforks.get(&neo_protocol::Hardfork::HfBasilisk),
            Some(&12)
        );
        assert_eq!(
            parse_duration_milliseconds(&serde_json::json!("15s")).unwrap(),
            15_000
        );
        assert!(parse_duration_milliseconds(&serde_json::json!("unknown")).is_err());
    }

    #[tokio::test]
    async fn unsupported_bolt_migration_fails_without_creating_target_storage() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("chain.db");
        std::fs::write(&source, b"not an empty source").unwrap();
        let target = temp.path().join("target");

        let error = convert_bolt_to_rocksdb(&source, &target, &ProtocolSettings::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not supported"));
        assert!(!target.join("RocksDBStore").exists());
    }
}

//! Auto-detection of Neo node implementation type.

use super::NodeImplementation;
use anyhow::{Result, bail};
use std::path::Path;

/// Detect which Neo implementation a directory belongs to.
///
/// Detection logic:
/// 1. neo-cli: config.json plus protocol.*.json, or a recognized Storage layout.
/// 2. neo-go: config.yml or config.yaml. Chain storage is not required for
///    configuration-only migration.
pub fn detect_implementation(source_dir: &Path) -> Result<NodeImplementation> {
    // Check neo-cli indicators
    let has_config_json = source_dir.join("config.json").exists();
    let has_protocol_json = source_dir
        .read_dir()?
        .filter_map(std::result::Result::ok)
        .any(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            name_str.starts_with("protocol.") && name_str.ends_with(".json")
        });
    let has_storage_dir = source_dir.join("Storage").exists();

    if has_config_json && has_protocol_json {
        return Ok(NodeImplementation::NeoCli);
    }

    // A neo-go protocol configuration is sufficient in configuration-only mode.
    let has_config_yml =
        source_dir.join("config.yml").exists() || source_dir.join("config.yaml").exists();
    if has_config_yml {
        return Ok(NodeImplementation::NeoGo);
    }

    // Try to detect by storage structure alone
    if has_storage_dir {
        // neo-cli uses Storage/LevelDBStore or Storage/RocksDBStore
        let leveldb = source_dir.join("Storage").join("LevelDBStore").exists();
        let rocksdb = source_dir.join("Storage").join("RocksDBStore").exists();
        if leveldb || rocksdb {
            return Ok(NodeImplementation::NeoCli);
        }
    }

    bail!(
        "Could not detect node implementation type in: {}\n\
        Expected neo-cli (config.json and protocol.*.json, or recognized Storage/) \
        or neo-go (config.yml or config.yaml)",
        source_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_detect_neo_cli() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Create neo-cli structure
        fs::write(dir.join("config.json"), "{}").unwrap();
        fs::write(dir.join("protocol.mainnet.json"), "{}").unwrap();
        fs::create_dir(dir.join("Storage")).unwrap();

        let impl_type = detect_implementation(dir).unwrap();
        assert_eq!(impl_type, NodeImplementation::NeoCli);
    }

    #[test]
    fn test_detect_neo_go() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();

        // Create neo-go structure
        fs::write(dir.join("config.yml"), "{}").unwrap();
        fs::write(dir.join("chain.db"), "").unwrap();

        let impl_type = detect_implementation(dir).unwrap();
        assert_eq!(impl_type, NodeImplementation::NeoGo);
    }

    #[test]
    fn detects_configuration_without_chain_storage() {
        let temp = TempDir::new().unwrap();
        let cli_dir = temp.path().join("cli");
        let go_dir = temp.path().join("go");
        fs::create_dir(&cli_dir).unwrap();
        fs::create_dir(&go_dir).unwrap();
        fs::write(cli_dir.join("config.json"), "{}").unwrap();
        fs::write(cli_dir.join("protocol.custom.json"), "{}").unwrap();
        fs::write(go_dir.join("config.yml"), "{}").unwrap();

        assert_eq!(
            detect_implementation(&cli_dir).unwrap(),
            NodeImplementation::NeoCli
        );
        assert_eq!(
            detect_implementation(&go_dir).unwrap(),
            NodeImplementation::NeoGo
        );
    }

    #[test]
    fn test_detect_fails_on_empty_dir() {
        let temp = TempDir::new().unwrap();
        let result = detect_implementation(temp.path());
        assert!(result.is_err());
    }
}

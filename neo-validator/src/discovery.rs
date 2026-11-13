//! Read-only discovery of upstream configuration. Upstream databases are never opened.

use crate::config::ValidatorConfig;
use anyhow::{Context, Result, bail};
use neo_protocol::ProtocolSettings;
use std::fs::File;
use std::path::{Path, PathBuf};

pub fn discover_protocol(config: &ValidatorConfig) -> Result<(ProtocolSettings, PathBuf)> {
    let expected_magic = config
        .network
        .magic
        .ok_or_else(|| anyhow::anyhow!("network.magic is required"))?;
    if let Some(path) = &config.network.protocol_path {
        let settings = load_protocol(path)?;
        if settings.network != expected_magic {
            bail!(
                "protocol network {} does not match configured magic {}",
                settings.network,
                expected_magic
            );
        }
        validate_directory_identity(config)?;
        return Ok((settings, path.clone()));
    }

    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(&config.neo_cli.path)
        .with_context(|| format!("failed to inspect {}", config.neo_cli.path.display()))?
    {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_file()
            && (name == "config.json" || (name.starts_with("protocol.") && name.ends_with(".json")))
        {
            candidates.push(path);
        }
    }
    candidates.sort();

    let mut matching = Vec::new();
    for path in candidates {
        if let Ok(settings) = load_protocol(&path)
            && settings.network == expected_magic
        {
            matching.push((settings, path));
        }
    }
    let Some((settings, path)) = matching.first().cloned() else {
        bail!(
            "no neo-cli config.json or protocol.*.json with network {} was found; set network.protocol_path explicitly",
            expected_magic
        );
    };
    if matching.iter().any(|(other, _)| other != &settings) {
        bail!("multiple different protocol files match network.magic; set protocol_path");
    }
    validate_directory_identity(config)?;
    Ok((settings, path))
}

fn load_protocol(path: &Path) -> Result<ProtocolSettings> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open protocol file {}", path.display()))?;
    ProtocolSettings::load_from_stream(&mut file)
        .map_err(|error| anyhow::anyhow!("invalid protocol file {}: {error}", path.display()))
}

fn validate_directory_identity(config: &ValidatorConfig) -> Result<()> {
    let cli = config
        .neo_cli
        .path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", config.neo_cli.path.display()))?;
    let go = config
        .neo_go
        .path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", config.neo_go.path.display()))?;
    if cli == go {
        bail!("neo_cli.path and neo_go.path must be different directories");
    }
    if !config.neo_go.path.join("config.yml").is_file()
        && !config.neo_go.path.join("config.yaml").is_file()
    {
        bail!("neo_go.path must contain config.yml or config.yaml");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_official_neo_cli_root_config() {
        let temp = tempfile::TempDir::new().unwrap();
        let cli = temp.path().join("neo-cli");
        let go = temp.path().join("neo-go");
        std::fs::create_dir_all(&cli).unwrap();
        std::fs::create_dir_all(&go).unwrap();
        std::fs::write(
            cli.join("config.json"),
            r#"{
                "ApplicationConfiguration": { "Logger": { "Path": "Logs" } },
                "ProtocolConfiguration": { "Network": 0 }
            }"#,
        )
        .unwrap();
        std::fs::write(
            go.join("config.yml"),
            "ProtocolConfiguration:\n  Magic: 0\n",
        )
        .unwrap();
        let mut config = ValidatorConfig::default();
        config.network.magic = Some(0);
        config.neo_cli.path = cli.clone();
        config.neo_go.path = go;

        let (settings, path) = discover_protocol(&config).unwrap();
        assert_eq!(path, cli.join("config.json"));
        assert_eq!(settings, ProtocolSettings::default());
    }
}

// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.

//! Explicit configuration loading, separate from normative protocol evaluation.
//!
//! These helpers preserve the legacy JSON, reader, and path behavior using only
//! the standard library. They do not require a host/core crate, start services,
//! or automatically inspect the environment when protocol settings are used.

use super::{Hardfork, ProtocolSettings, parse_committee};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub(super) fn find_file(file_name: &str, path: &str) -> Option<String> {
    let primary_root = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        application_root()
            .map(|root| root.join(path))
            .unwrap_or_else(|| PathBuf::from(path))
    };

    let primary = primary_root.join(file_name);
    if primary.exists() {
        return Some(primary.to_string_lossy().to_string());
    }

    if let Some(exec_root) = application_root() {
        let fallback = exec_root.join(file_name);
        if fallback.exists() {
            return Some(fallback.to_string_lossy().to_string());
        }
    }

    None
}

pub(super) fn load_from_stream(stream: &mut dyn Read) -> Result<ProtocolSettings, String> {
    // Consume from the current position; callers need not implement Seek.
    let mut buffered = Vec::new();
    stream
        .read_to_end(&mut buffered)
        .map_err(|err| format!("Failed to read protocol settings stream: {}", err))?;

    if buffered.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(ProtocolSettings::default());
    }

    let value: Value = serde_json::from_slice(&buffered)
        .map_err(|err| format!("Invalid protocol settings JSON: {}", err))?;
    from_value(value)
}

pub(super) fn load(path: &str) -> Result<ProtocolSettings, String> {
    let resolved_path = {
        let base_dir = std::env::current_dir()
            .ok()
            .and_then(|dir| dir.to_str().map(|s| s.to_string()));
        match base_dir {
            Some(base) => find_file(path, &base).unwrap_or_else(|| path.to_string()),
            None => path.to_string(),
        }
    };

    if !Path::new(&resolved_path).exists() {
        return Ok(ProtocolSettings::default());
    }

    let mut file = File::open(&resolved_path)
        .map_err(|err| format!("Failed to open protocol settings file: {}", err))?;
    // Ensure the stream cursor sits at the beginning for delegates expecting fresh readers.
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("Failed to rewind protocol settings file: {}", err))?;
    load_from_stream(&mut file)
}

fn application_root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn from_value(value: Value) -> Result<ProtocolSettings, String> {
    if value.is_null() {
        return Ok(ProtocolSettings::default());
    }

    let section = match value {
        Value::Object(mut map) => map
            .remove("ProtocolConfiguration")
            .unwrap_or(Value::Object(map)),
        other => other,
    };

    let raw: ProtocolConfiguration =
        serde_json::from_value(section).map_err(|err| err.to_string())?;
    from_raw(raw)
}

fn from_raw(raw: ProtocolConfiguration) -> Result<ProtocolSettings, String> {
    let mut settings = ProtocolSettings::default_settings();

    if let Some(network) = raw.network {
        settings.network = network;
    }
    if let Some(version) = raw.address_version {
        settings.address_version = version;
    }
    if let Some(validators) = raw.validators_count {
        settings.validators_count = validators;
    }
    if let Some(list) = raw.seed_list {
        settings.seed_list = list;
    }
    if let Some(ms_per_block) = raw.milliseconds_per_block {
        settings.milliseconds_per_block = ms_per_block;
    }
    if let Some(max_tx) = raw.max_transactions_per_block {
        settings.max_transactions_per_block = max_tx;
    }
    if let Some(max_valid) = raw.max_valid_until_block_increment {
        settings.max_valid_until_block_increment = max_valid;
    }
    if let Some(max_mempool) = raw.memory_pool_max_transactions {
        settings.memory_pool_max_transactions = max_mempool;
    }
    if let Some(max_traceable) = raw.max_traceable_blocks {
        settings.max_traceable_blocks = max_traceable;
    }
    if let Some(initial_gas) = raw.initial_gas_distribution {
        settings.initial_gas_distribution = initial_gas;
    }

    if let Some(committee) = raw.standby_committee {
        settings.standby_committee = parse_committee(committee)?;
    }

    if let Some(hardforks) = raw.hardforks {
        let mut parsed = HashMap::new();
        for (name, height) in hardforks {
            let hardfork = Hardfork::from_str(&name).map_err(|err| format!("{}: {}", name, err))?;
            parsed.insert(hardfork, height);
        }

        settings.hardforks = ProtocolSettings::ensure_omitted_hardforks(parsed);
        ProtocolSettings::validate_hardfork_sequence(&settings.hardforks)?;
    }

    Ok(settings)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProtocolConfiguration {
    #[serde(default)]
    network: Option<u32>,
    #[serde(default)]
    address_version: Option<u8>,
    #[serde(default)]
    standby_committee: Option<Vec<String>>,
    #[serde(default)]
    validators_count: Option<i32>,
    #[serde(default)]
    seed_list: Option<Vec<String>>,
    #[serde(default)]
    milliseconds_per_block: Option<u32>,
    #[serde(default)]
    max_valid_until_block_increment: Option<u32>,
    #[serde(default)]
    max_transactions_per_block: Option<u32>,
    #[serde(default)]
    memory_pool_max_transactions: Option<i32>,
    #[serde(default)]
    max_traceable_blocks: Option<u32>,
    #[serde(default)]
    hardforks: Option<HashMap<String, u32>>,
    #[serde(default)]
    initial_gas_distribution: Option<u64>,
}

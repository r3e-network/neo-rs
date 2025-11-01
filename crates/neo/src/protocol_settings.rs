// Copyright (C) 2015-2025 The Neo Project.
//
// protocol_settings.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    constants,
    cryptography::crypto_utils::ECPoint,
    hardfork::{Hardfork, HardforkManager},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// Represents the protocol settings of the NEO system.
/// Matches C# ProtocolSettings record exactly
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolSettings {
    /// The magic number of the NEO network.
    /// Matches C# Network property
    pub network: u32,

    /// The address version of the NEO system.
    /// Matches C# AddressVersion property
    pub address_version: u8,

    /// The public keys of the standby committee members.
    /// Matches C# StandbyCommittee property
    pub standby_committee: Vec<ECPoint>,

    /// The number of the validators in NEO system.
    /// Matches C# ValidatorsCount property
    pub validators_count: i32,

    /// The default seed nodes list.
    /// Matches C# SeedList property
    pub seed_list: Vec<String>,

    /// Indicates the time in milliseconds between two blocks.
    /// Matches C# MillisecondsPerBlock property
    pub milliseconds_per_block: u32,

    /// The maximum increment of the ValidUntilBlock field.
    /// Matches C# MaxValidUntilBlockIncrement property
    pub max_valid_until_block_increment: u32,

    /// Indicates the maximum number of transactions that can be contained in a block.
    /// Matches C# MaxTransactionsPerBlock property
    pub max_transactions_per_block: u32,

    /// Indicates the maximum number of transactions that can be contained in the memory pool.
    /// Matches C# MemoryPoolMaxTransactions property
    pub memory_pool_max_transactions: i32,

    /// Indicates the maximum number of blocks that can be traced in the smart contract.
    /// Matches C# MaxTraceableBlocks property
    pub max_traceable_blocks: u32,

    /// Sets the block height from which a hardfork is activated.
    /// Matches C# Hardforks property
    pub hardforks: HashMap<Hardfork, u32>,

    /// Indicates the amount of gas to distribute during initialization.
    /// Matches C# InitialGasDistribution property
    pub initial_gas_distribution: u64,
}

impl ProtocolSettings {
    /// The number of members of the committee in NEO system.
    /// Matches C# CommitteeMembersCount property
    pub fn committee_members_count(&self) -> usize {
        self.standby_committee.len()
    }

    /// Indicates the time between two blocks.
    /// Matches C# TimePerBlock property
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(self.milliseconds_per_block as u64)
    }

    /// The public keys of the standby validators.
    /// Matches C# StandbyValidators property
    pub fn standby_validators(&self) -> Vec<ECPoint> {
        self.standby_committee
            .iter()
            .take(self.validators_count as usize)
            .cloned()
            .collect()
    }

    /// The default protocol settings for NEO MainNet.
    /// Matches C# Default property
    pub fn default_settings() -> Self {
        Self {
            network: 0,
            address_version: constants::ADDRESS_VERSION,
            standby_committee: Vec::new(),
            validators_count: 0,
            seed_list: Vec::new(),
            milliseconds_per_block: constants::MILLISECONDS_PER_BLOCK as u32,
            max_transactions_per_block: constants::MAX_TRANSACTIONS_PER_BLOCK as u32,
            max_valid_until_block_increment: 86_400_000
                / (constants::MILLISECONDS_PER_BLOCK as u32),
            memory_pool_max_transactions: constants::MEMORY_POOL_MAX_TRANSACTIONS as i32,
            max_traceable_blocks: constants::MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: constants::INITIAL_GAS_DISTRIBUTION,
            hardforks: Self::ensure_omitted_hardforks(HashMap::new()),
        }
    }

    /// Returns whether the provided hardfork is enabled at the given block height.
    /// Mirrors C# `ProtocolSettings.IsHardforkEnabled`.
    pub fn is_hardfork_enabled(&self, hardfork: Hardfork, block_height: u32) -> bool {
        self.hardforks
            .get(&hardfork)
            .map(|&activation_height| block_height >= activation_height)
            .unwrap_or(false)
    }

    /// Searches for a file in the given path. If not found, checks in the executable directory.
    /// Matches C# FindFile method
    pub fn find_file(file_name: &str, path: &str) -> Option<String> {
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

    /// Loads the ProtocolSettings from the specified stream.
    /// Matches C# Load(Stream) method
    pub fn load_from_stream(stream: &mut dyn Read) -> Result<Self, String> {
        // serde_json::from_reader consumes the stream; seek back to handle reuse over same stream.
        let mut buffered = Vec::new();
        stream
            .read_to_end(&mut buffered)
            .map_err(|err| format!("Failed to read protocol settings stream: {}", err))?;

        if buffered.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Ok(Self::default());
        }

        let value: Value = serde_json::from_slice(&buffered)
            .map_err(|err| format!("Invalid protocol settings JSON: {}", err))?;
        Self::from_value(value)
    }

    /// Loads the ProtocolSettings at the specified path.
    /// Matches C# Load(string) method
    pub fn load(path: &str) -> Result<Self, String> {
        let resolved_path =
            Self::find_file(path, std::env::current_dir().unwrap().to_str().unwrap())
                .unwrap_or_else(|| path.to_string());

        if !Path::new(&resolved_path).exists() {
            return Ok(Self::default());
        }

        let mut file = File::open(&resolved_path)
            .map_err(|err| format!("Failed to open protocol settings file: {}", err))?;
        // Ensure the stream cursor sits at the beginning for delegates expecting fresh readers.
        file.seek(SeekFrom::Start(0))
            .map_err(|err| format!("Failed to rewind protocol settings file: {}", err))?;
        Self::load_from_stream(&mut file)
    }

    /// Ensures omitted hardforks are included.
    /// Matches C# EnsureOmmitedHardforks method
    fn ensure_omitted_hardforks(hardforks: HashMap<Hardfork, u32>) -> HashMap<Hardfork, u32> {
        let mut hardforks = hardforks;
        let mut encountered_configured = false;
        for hardfork in HardforkManager::all() {
            if hardforks.contains_key(&hardfork) {
                encountered_configured = true;
            } else if !encountered_configured {
                hardforks.insert(hardfork, 0);
            } else {
                break;
            }
        }
        hardforks
    }

    fn from_value(value: Value) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self::default());
        }

        let section = match value {
            Value::Object(mut map) => map
                .remove("ProtocolConfiguration")
                .unwrap_or(Value::Object(map)),
            other => other,
        };

        let raw: ProtocolConfiguration =
            serde_json::from_value(section).map_err(|err| err.to_string())?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: ProtocolConfiguration) -> Result<Self, String> {
        let mut settings = Self::default_settings();

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
                let hardfork =
                    Hardfork::from_str(&name).map_err(|err| format!("{}: {}", name, err))?;
                parsed.insert(hardfork, height);
            }

            settings.hardforks = Self::ensure_omitted_hardforks(parsed);
            Self::validate_hardfork_sequence(&settings.hardforks)?;
        }

        Ok(settings)
    }

    fn validate_hardfork_sequence(hardforks: &HashMap<Hardfork, u32>) -> Result<(), String> {
        let all = HardforkManager::all();
        let mut previous_index: Option<usize> = None;
        let mut previous_height: Option<u32> = None;

        for (index, hardfork) in all.iter().enumerate() {
            if let Some(&height) = hardforks.get(hardfork) {
                if let Some(prev_index) = previous_index {
                    if index - prev_index > 1 {
                        let missing = all[prev_index + 1];
                        return Err(format!(
                            "Hardfork {:?} is configured while {:?} is missing. Configure every hardfork sequentially.",
                            hardfork, missing
                        ));
                    }
                }

                if let Some(prev_height) = previous_height {
                    if height < prev_height {
                        return Err(format!(
                            "Hardfork {:?} activates at block {}, which is before previously configured height {}.",
                            hardfork, height, prev_height
                        ));
                    }
                }

                previous_index = Some(index);
                previous_height = Some(height);
            }
        }

        Ok(())
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

fn application_root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn parse_committee(entries: Vec<String>) -> Result<Vec<ECPoint>, String> {
    let mut committee = Vec::with_capacity(entries.len());
    for entry in entries {
        let trimmed = entry
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        if trimmed.is_empty() {
            continue;
        }
        let bytes = hex::decode(trimmed)
            .map_err(|err| format!("Invalid ECPoint hex '{}': {}", entry, err))?;
        let point = ECPoint::from_bytes(&bytes)?;
        committee.push(point);
    }
    Ok(committee)
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

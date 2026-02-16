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
    cryptography::ECPoint,
    hardfork::{Hardfork, HardforkManager},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, hash_map::Entry};
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

    /// Indicates the maximum size of a block in bytes.
    /// Matches C# MaxBlockSize property
    pub max_block_size: u32,

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

    /// Returns built-in ProtocolSettings for Neo MainNet.
    pub fn mainnet() -> Self {
        let standby_committee = parse_committee_slice(&[
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
            "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554",
            "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d",
            "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e",
            "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70",
            "023a36c72844610b4d34d1968662424011bf783ca9d984efa19a20babf5582f3fe",
            "03708b860c1de5d87f5b151a12c2a99feebd2e8b315ee8e7cf8aa19692a9e18379",
            "03c6aa6e12638b36e88adc1ccdceac4db9929575c3e03576c617c49cce7114a050",
            "03204223f8c86b8cd5c89ef12e4f0dbb314172e9241e30c9ef2293790793537cf0",
            "02a62c915cf19c7f19a50ec217e79fac2439bbaad658493de0c7d8ffa92ab0aa62",
            "03409f31f0d66bdc2f70a9730b66fe186658f84a8018204db01c106edc36553cd0",
            "0288342b141c30dc8ffcde0204929bb46aed5756b41ef4a56778d15ada8f0c6654",
            "020f2887f41474cfeb11fd262e982051c1541418137c02a0f4961af911045de639",
            "0222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c30",
            "03d281b42002647f0113f36c7b8efb30db66078dfaaa9ab3ff76d043a98d512fde",
            "02504acbc1f4b3bdad1d86d6e1a08603771db135a73e61c9d565ae06a1938cd2ad",
            "0226933336f1b75baa42d42b71d9091508b638046d19abd67f4e119bf64a7cfb4d",
            "03cdcea66032b82f5c30450e381e5295cae85c5e6943af716cc6b646352a6067dc",
            "02cd5a5547119e24feaa7c2a0f37b8c9366216bab7054de0065c9be42084003c8a",
        ])
        .expect("embedded mainnet committee should be valid");

        let hardforks = HardforkManager::mainnet().get_hardforks().clone();

        Self {
            network: constants::MAINNET_MAGIC,
            address_version: constants::ADDRESS_VERSION,
            standby_committee,
            validators_count: 7,
            seed_list: vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
            ],
            milliseconds_per_block: 15_000,
            max_transactions_per_block: 512,
            max_block_size: constants::MAX_BLOCK_SIZE as u32,
            max_valid_until_block_increment: 5_760,
            memory_pool_max_transactions: 50_000,
            max_traceable_blocks: constants::MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: constants::INITIAL_GAS_DISTRIBUTION,
            hardforks: Self::ensure_omitted_hardforks(hardforks),
        }
    }

    /// Returns built-in ProtocolSettings for Neo TestNet.
    pub fn testnet() -> Self {
        let standby_committee = parse_committee_slice(&[
            "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d",
            "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2",
            "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd",
            "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806",
            "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b",
            "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01",
            "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba",
            "025831cee3708e87d78211bec0d1bfee9f4c85ae784762f042e7f31c0d40c329b8",
            "02cf9dc6e85d581480d91e88e8cbeaa0c153a046e89ded08b4cefd851e1d7325b5",
            "03840415b0a0fcf066bcc3dc92d8349ebd33a6ab1402ef649bae00e5d9f5840828",
            "026328aae34f149853430f526ecaa9cf9c8d78a4ea82d08bdf63dd03c4d0693be6",
            "02c69a8d084ee7319cfecf5161ff257aa2d1f53e79bf6c6f164cff5d94675c38b3",
            "0207da870cedb777fceff948641021714ec815110ca111ccc7a54c168e065bda70",
            "035056669864feea401d8c31e447fb82dd29f342a9476cfd449584ce2a6165e4d7",
            "0370c75c54445565df62cfe2e76fbec4ba00d1298867972213530cae6d418da636",
            "03957af9e77282ae3263544b7b2458903624adc3f5dee303957cb6570524a5f254",
            "03d84d22b8753cf225d263a3a782a4e16ca72ef323cfde04977c74f14873ab1e4c",
            "02147c1b1d5728e1954958daff2f88ee2fa50a06890a8a9db3fa9e972b66ae559f",
            "03c609bea5a4825908027e4ab217e7efc06e311f19ecad9d417089f14927a173d5",
            "0231edee3978d46c335e851c76059166eb8878516f459e085c0dd092f0f1d51c21",
            "03184b018d6b2bc093e535519732b3fd3f7551c8cffaf4621dd5a0b89482ca66c9",
        ])
        .expect("embedded testnet committee should be valid");

        let hardforks = HardforkManager::testnet().get_hardforks().clone();

        Self {
            network: constants::TESTNET_MAGIC,
            address_version: constants::ADDRESS_VERSION,
            standby_committee,
            validators_count: 7,
            seed_list: vec![
                "seed1t5.neo.org:20333".to_string(),
                "seed2t5.neo.org:20333".to_string(),
                "seed3t5.neo.org:20333".to_string(),
                "seed4t5.neo.org:20333".to_string(),
                "seed5t5.neo.org:20333".to_string(),
            ],
            milliseconds_per_block: 15_000,
            max_transactions_per_block: 512,
            max_block_size: constants::MAX_BLOCK_SIZE as u32,
            max_valid_until_block_increment: 5_760,
            memory_pool_max_transactions: 50_000,
            max_traceable_blocks: constants::MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: constants::INITIAL_GAS_DISTRIBUTION,
            hardforks: Self::ensure_omitted_hardforks(hardforks),
        }
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
        Self::mainnet()
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
        let resolved_path = {
            let base_dir = std::env::current_dir()
                .ok()
                .and_then(|dir| dir.to_str().map(|s| s.to_string()));
            match base_dir {
                Some(base) => Self::find_file(path, &base).unwrap_or_else(|| path.to_string()),
                None => path.to_string(),
            }
        };

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
            match hardforks.entry(hardfork) {
                Entry::Occupied(_) => encountered_configured = true,
                Entry::Vacant(entry) if !encountered_configured => {
                    entry.insert(0);
                }
                _ => break,
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

fn parse_committee_slice(entries: &[&str]) -> Result<Vec<ECPoint>, String> {
    parse_committee(entries.iter().map(|entry| entry.to_string()).collect())
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
        let point = ECPoint::from_bytes(&bytes).map_err(|e| e.to_string())?;
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

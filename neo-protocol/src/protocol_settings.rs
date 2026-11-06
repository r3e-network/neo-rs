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

//! Normative protocol parameters and activation rules.
//!
//! Network defaults remain owned by `neo-config`; elliptic curve points and
//! hardfork identities remain owned by `neo-crypto` and `neo-primitives`.
//! File discovery and JSON loading are isolated in the private `loading` module.
//! They are explicit convenience entry points, not inputs implicitly consulted
//! by protocol rules. No host, node, or core crate is required.

mod loading;

use crate::hardfork::{Hardfork, HardforkManager, hardfork_height};
use neo_crypto::ECPoint;
use neo_primitives::constants;
use std::collections::{HashMap, hash_map::Entry};
use std::io::Read;
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

    /// Converts the shared pure-data configuration into normative protocol parameters.
    fn from_config(config: neo_config::ProtocolSettings) -> Result<Self, String> {
        let standby_committee = parse_committee(config.standby_validators)?;
        let validators_count = i32::try_from(config.validators_count)
            .map_err(|_| "validators_count exceeds i32".to_string())?;
        let milliseconds_per_block = u32::try_from(config.ms_per_block)
            .map_err(|_| "ms_per_block exceeds u32".to_string())?;
        let memory_pool_max_transactions = i32::try_from(config.memory_pool_max_transactions)
            .map_err(|_| "memory_pool_max_transactions exceeds i32".to_string())?;
        let initial_gas_distribution = u64::try_from(config.initial_gas_distribution)
            .map_err(|_| "initial_gas_distribution must be non-negative".to_string())?;
        let mut hardforks = HashMap::new();
        for hardfork in Hardfork::all() {
            if let Some(height) = hardfork_height(&config.hardforks, hardfork) {
                hardforks.insert(hardfork, height);
            }
        }
        let hardforks = Self::ensure_omitted_hardforks(hardforks);
        Ok(Self {
            network: config.network,
            address_version: config.address_version,
            standby_committee,
            validators_count,
            seed_list: config.seed_list,
            milliseconds_per_block,
            max_transactions_per_block: config.max_transactions_per_block,
            max_block_size: constants::MAX_BLOCK_SIZE as u32,
            max_valid_until_block_increment: config.max_valid_until_block_increment,
            memory_pool_max_transactions,
            max_traceable_blocks: config.max_traceable_blocks,
            hardforks,
            initial_gas_distribution,
        })
    }

    /// Returns built-in ProtocolSettings for Neo MainNet.
    pub fn mainnet() -> Self {
        Self::from_config(neo_config::ProtocolSettings::mainnet())
            .expect("embedded mainnet protocol settings should be valid")
    }

    /// Returns built-in ProtocolSettings for Neo TestNet.
    pub fn testnet() -> Self {
        Self::from_config(neo_config::ProtocolSettings::testnet())
            .expect("embedded testnet protocol settings should be valid")
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

    /// The fallback settings, matching C# `ProtocolSettings.Default`:
    /// `Network = 0`, empty committee and seed list, `ValidatorsCount = 0`,
    /// and every omitted hardfork enabled at genesis.
    ///
    /// This is not `neo_config::ProtocolSettings::default()`, which selects
    /// MainNet. MainNet/TestNet parameters must be requested explicitly via
    /// [`Self::mainnet`] / [`Self::testnet`] or loaded from a configuration file.
    pub fn default_settings() -> Self {
        Self {
            network: 0,
            address_version: constants::ADDRESS_VERSION,
            standby_committee: Vec::new(),
            validators_count: 0,
            seed_list: Vec::new(),
            milliseconds_per_block: 15_000,
            max_transactions_per_block: 512,
            max_block_size: constants::MAX_BLOCK_SIZE as u32,
            max_valid_until_block_increment: 5_760,
            memory_pool_max_transactions: 50_000,
            max_traceable_blocks: constants::MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: constants::INITIAL_GAS_DISTRIBUTION,
            // C# EnsureOmmitedHardforks fills the leading omitted forks with 0.
            // With no configured fork this includes every known fork.
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

    /// Returns whether the hardfork is configured (has an activation height defined),
    /// regardless of whether it has been activated yet. Used for native contract state
    /// generation — methods from defined hardforks are included in the contract's
    /// NEF/ABI even before their activation height.
    pub fn is_hardfork_defined(&self, hardfork: Hardfork) -> bool {
        self.hardforks.contains_key(&hardfork)
    }

    /// Explicit file discovery, retaining the C# FindFile-compatible signature.
    /// Relative search roots are executable-relative, with executable fallback.
    /// This loading convenience does not participate in normative protocol rules.
    pub fn find_file(file_name: &str, path: &str) -> Option<String> {
        loading::find_file(file_name, path)
    }

    /// Explicit JSON loading from the reader's current position.
    /// Retains the C# Load(Stream)-compatible signature and string errors.
    /// Parsing/loading is separate from normative protocol data and activation.
    pub fn load_from_stream(stream: &mut dyn Read) -> Result<Self, String> {
        loading::load_from_stream(stream)
    }

    /// Explicit file loading, retaining the C# Load(string)-compatible signature.
    /// Missing files produce fallback settings, never implicit MainNet settings.
    /// File access is confined to the loading module, not protocol evaluation.
    pub fn load(path: &str) -> Result<Self, String> {
        loading::load(path)
    }

    /// Ensures omitted hardforks are included.
    /// Matches C# EnsureOmmitedHardforks method
    fn ensure_omitted_hardforks(hardforks: HashMap<Hardfork, u32>) -> HashMap<Hardfork, u32> {
        let mut hardforks = hardforks;
        let mut encountered_configured = false;
        for &hardfork in HardforkManager::all() {
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

    fn validate_hardfork_sequence(hardforks: &HashMap<Hardfork, u32>) -> Result<(), String> {
        let all = HardforkManager::all();
        let mut previous_index: Option<usize> = None;
        let mut previous_height: Option<u32> = None;

        for (index, hardfork) in all.iter().enumerate() {
            if let Some(&height) = hardforks.get(hardfork) {
                if let Some(prev_index) = previous_index
                    && index - prev_index > 1
                {
                    let missing = all[prev_index + 1];
                    return Err(format!(
                        "Hardfork {:?} is configured while {:?} is missing. Configure every hardfork sequentially.",
                        hardfork, missing
                    ));
                }

                if let Some(prev_height) = previous_height
                    && height < prev_height
                {
                    return Err(format!(
                        "Hardfork {:?} activates at block {}, which is before previously configured height {}.",
                        hardfork, height, prev_height
                    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_numeric_boundaries_preserve_field_widths() {
        let mut config = neo_config::ProtocolSettings::private(42);
        config.validators_count = i32::MAX as u32;
        config.ms_per_block = u32::MAX as u64;
        config.memory_pool_max_transactions = i32::MAX as u32;
        config.initial_gas_distribution = i64::MAX;
        let settings = ProtocolSettings::from_config(config).expect("boundary settings");
        assert_eq!(settings.validators_count, i32::MAX);
        assert_eq!(settings.milliseconds_per_block, u32::MAX);
        assert_eq!(settings.memory_pool_max_transactions, i32::MAX);
        assert_eq!(settings.initial_gas_distribution, i64::MAX as u64);
    }

    #[test]
    fn config_numeric_overflow_errors_are_preserved() {
        let mut config = neo_config::ProtocolSettings::private(42);
        config.validators_count = i32::MAX as u32 + 1;
        assert_eq!(
            ProtocolSettings::from_config(config).unwrap_err(),
            "validators_count exceeds i32"
        );

        let mut config = neo_config::ProtocolSettings::private(42);
        config.ms_per_block = u32::MAX as u64 + 1;
        assert_eq!(
            ProtocolSettings::from_config(config).unwrap_err(),
            "ms_per_block exceeds u32"
        );

        let mut config = neo_config::ProtocolSettings::private(42);
        config.memory_pool_max_transactions = i32::MAX as u32 + 1;
        assert_eq!(
            ProtocolSettings::from_config(config).unwrap_err(),
            "memory_pool_max_transactions exceeds i32"
        );

        let mut config = neo_config::ProtocolSettings::private(42);
        config.initial_gas_distribution = -1;
        assert_eq!(
            ProtocolSettings::from_config(config).unwrap_err(),
            "initial_gas_distribution must be non-negative"
        );
    }
}

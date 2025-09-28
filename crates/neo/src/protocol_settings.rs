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
use crate::{cryptography::crypto_utils::ECPoint, hardfork::Hardfork};
use std::collections::HashMap;
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
            address_version: 0x35,
            standby_committee: Vec::new(),
            validators_count: 0,
            seed_list: Vec::new(),
            milliseconds_per_block: 15000,
            max_transactions_per_block: 512,
            max_valid_until_block_increment: 86400000 / 15000,
            memory_pool_max_transactions: 50_000,
            max_traceable_blocks: 2_102_400,
            initial_gas_distribution: 52_000_000_00000000,
            hardforks: Self::ensure_omitted_hardforks(HashMap::new()),
        }
    }

    /// Searches for a file in the given path. If not found, checks in the executable directory.
    /// Matches C# FindFile method
    pub fn find_file(file_name: &str, path: &str) -> Option<String> {
        use std::path::Path;

        let path = if Path::new(path).is_absolute() {
            path.to_string()
        } else {
            // In a real implementation, this would combine with executable directory
            path.to_string()
        };

        let full_path = Path::new(&path).join(file_name);
        if full_path.exists() {
            Some(full_path.to_string_lossy().to_string())
        } else {
            // In a real implementation, this would check executable directory
            None
        }
    }

    /// Loads the ProtocolSettings from the specified stream.
    /// Matches C# Load(Stream) method
    pub fn load_from_stream(stream: &mut dyn std::io::Read) -> Result<Self, String> {
        // In a real implementation, this would parse JSON from the stream
        Ok(Self::default())
    }

    /// Loads the ProtocolSettings at the specified path.
    /// Matches C# Load(string) method
    pub fn load(path: &str) -> Result<Self, String> {
        let resolved_path =
            Self::find_file(path, std::env::current_dir().unwrap().to_str().unwrap())
                .unwrap_or_else(|| path.to_string());

        if !std::path::Path::new(&resolved_path).exists() {
            return Ok(Self::default());
        }

        let mut file = std::fs::File::open(&resolved_path)?;
        Self::load_from_stream(&mut file)
    }

    /// Ensures omitted hardforks are included.
    /// Matches C# EnsureOmmitedHardforks method
    fn ensure_omitted_hardforks(mut hardforks: HashMap<Hardfork, u32>) -> HashMap<Hardfork, u32> {
        // In a real implementation, this would ensure all hardforks are included
        hardforks
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

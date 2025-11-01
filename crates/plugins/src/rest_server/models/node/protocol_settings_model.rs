// Copyright (C) 2015-2025 The Neo Project.
//
// protocol_settings_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Protocol settings model matching C# ProtocolSettingsModel exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSettingsModel {
    /// Network
    /// Matches C# Network property
    pub network: u32,

    /// AddressVersion
    /// Matches C# AddressVersion property
    pub address_version: u8,

    /// ValidatorsCount
    /// Matches C# ValidatorsCount property
    pub validators_count: i32,

    /// MillisecondsPerBlock
    /// Matches C# MillisecondsPerBlock property
    pub milliseconds_per_block: u32,

    /// MaxValidUntilBlockIncrement
    /// Matches C# MaxValidUntilBlockIncrement property
    pub max_valid_until_block_increment: u32,

    /// MaxTransactionsPerBlock
    /// Matches C# MaxTransactionsPerBlock property
    pub max_transactions_per_block: u32,

    /// MemoryPoolMaxTransactions
    /// Matches C# MemoryPoolMaxTransactions property
    pub memory_pool_max_transactions: i32,

    /// MaxTraceableBlocks
    /// Matches C# MaxTraceableBlocks property
    pub max_traceable_blocks: u32,

    /// InitialGasDistribution
    /// Matches C# InitialGasDistribution property
    pub initial_gas_distribution: u64,

    /// SeedList
    /// Matches C# SeedList property
    pub seed_list: Vec<String>,

    /// Hardforks
    /// Matches C# Hardforks property
    pub hardforks: HashMap<String, u32>,

    /// StandbyValidators
    /// Matches C# StandbyValidators property
    pub standby_validators: Vec<String>,

    /// StandbyCommittee
    /// Matches C# StandbyCommittee property
    pub standby_committee: Vec<String>,
}

impl ProtocolSettingsModel {
    /// Creates a new ProtocolSettingsModel
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            network: 0,
            address_version: 0,
            validators_count: 0,
            milliseconds_per_block: 0,
            max_valid_until_block_increment: 0,
            max_transactions_per_block: 0,
            memory_pool_max_transactions: 0,
            max_traceable_blocks: 0,
            initial_gas_distribution: 0,
            seed_list: Vec::new(),
            hardforks: HashMap::new(),
            standby_validators: Vec::new(),
            standby_committee: Vec::new(),
        }
    }

    /// Creates a new ProtocolSettingsModel with parameters
    /// Matches C# constructor with parameters
    pub fn with_params(
        network: u32,
        address_version: u8,
        validators_count: i32,
        milliseconds_per_block: u32,
        max_valid_until_block_increment: u32,
        max_transactions_per_block: u32,
        memory_pool_max_transactions: i32,
        max_traceable_blocks: u32,
        initial_gas_distribution: u64,
        seed_list: Vec<String>,
        hardforks: HashMap<String, u32>,
        standby_validators: Vec<String>,
        standby_committee: Vec<String>,
    ) -> Self {
        Self {
            network,
            address_version,
            validators_count,
            milliseconds_per_block,
            max_valid_until_block_increment,
            max_transactions_per_block,
            memory_pool_max_transactions,
            max_traceable_blocks,
            initial_gas_distribution,
            seed_list,
            hardforks,
            standby_validators,
            standby_committee,
        }
    }
}

impl Default for ProtocolSettingsModel {
    fn default() -> Self {
        Self::new()
    }
}

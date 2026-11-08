// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Embedded protocol configuration data (mainnet, testnet).
//!
//! This module contains the configuration types that were previously in neo-config.
//! After the crate consolidation, these are now internal to neo-protocol.

/// Hardfork activation heights in a protocol configuration.
#[derive(Debug, Clone, Default)]
pub struct HardforkHeights {
    pub hf_aspidochelone: Option<u32>,
    pub hf_basilisk: Option<u32>,
    pub hf_cockatrice: Option<u32>,
    pub hf_domovoi: Option<u32>,
    pub hf_echidna: Option<u32>,
    pub hf_faun: Option<u32>,
    pub hf_gorgon: Option<u32>,
    pub hf_huyao: Option<u32>,
}

/// Raw protocol settings data structure.
#[derive(Debug, Clone)]
pub struct ProtocolSettings {
    pub network: u32,
    pub address_version: u8,
    pub standby_validators: Vec<String>,
    pub validators_count: u32,
    pub seed_list: Vec<String>,
    pub ms_per_block: u64,
    pub max_transactions_per_block: u32,
    pub max_valid_until_block_increment: u32,
    pub memory_pool_max_transactions: u32,
    pub max_traceable_blocks: u32,
    pub hardforks: HardforkHeights,
    pub initial_gas_distribution: i64,
}

impl ProtocolSettings {
    /// Returns the embedded MainNet configuration.
    pub fn mainnet() -> Self {
        Self {
            network: 860833102,
            address_version: 53,
            standby_validators: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a".to_string(),
                "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554".to_string(),
                "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d".to_string(),
                "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e".to_string(),
                "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70".to_string(),
            ],
            validators_count: 7,
            seed_list: vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
            ],
            ms_per_block: 15000,
            max_transactions_per_block: 512,
            max_valid_until_block_increment: 5760,
            memory_pool_max_transactions: 50000,
            max_traceable_blocks: 2102400,
            hardforks: HardforkHeights {
                hf_aspidochelone: Some(1730000),
                hf_basilisk: Some(4120000),
                hf_cockatrice: Some(5465000),
                hf_domovoi: Some(6900000),
                hf_echidna: Some(7300000),
                hf_faun: Some(8800000),
                hf_gorgon: Some(12020000),
                hf_huyao: None,
            },
            initial_gas_distribution: 52000000_00000000,
        }
    }

    /// Returns the embedded TestNet configuration.
    pub fn testnet() -> Self {
        Self {
            network: 894710606,
            address_version: 53,
            standby_validators: vec![
                "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d".to_string(),
                "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2".to_string(),
                "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd".to_string(),
                "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806".to_string(),
                "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b".to_string(),
                "0214baf0ceea3a66f17a7b9b9e4ffeae88181962e9688be11f3c2b3a78f38ae042".to_string(),
                "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba".to_string(),
            ],
            validators_count: 7,
            seed_list: vec![
                "seed1t5.neo.org:20333".to_string(),
                "seed2t5.neo.org:20333".to_string(),
                "seed3t5.neo.org:20333".to_string(),
                "seed4t5.neo.org:20333".to_string(),
                "seed5t5.neo.org:20333".to_string(),
            ],
            ms_per_block: 15000,
            max_transactions_per_block: 512,
            max_valid_until_block_increment: 5760,
            memory_pool_max_transactions: 50000,
            max_traceable_blocks: 2102400,
            hardforks: HardforkHeights {
                hf_aspidochelone: Some(210000),
                hf_basilisk: Some(2680000),
                hf_cockatrice: Some(4180000),
                hf_domovoi: Some(6020000),
                hf_echidna: Some(5870000),
                hf_faun: Some(12960000),
                hf_gorgon: Some(17960000),
                hf_huyao: None,
            },
            initial_gas_distribution: 52000000_00000000,
        }
    }

    /// Creates a private network configuration for testing.
    #[cfg(test)]
    pub fn private(network: u32) -> Self {
        Self {
            network,
            address_version: 53,
            standby_validators: vec![],
            validators_count: 0,
            seed_list: vec![],
            ms_per_block: 15000,
            max_transactions_per_block: 512,
            max_valid_until_block_increment: 5760,
            memory_pool_max_transactions: 50000,
            max_traceable_blocks: 2102400,
            hardforks: HardforkHeights::default(),
            initial_gas_distribution: 52000000_00000000,
        }
    }
}

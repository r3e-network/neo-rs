//! Protocol settings for Neo N3 blockchain
//!
//! These settings define the core protocol parameters that must match
//! the Neo N3 C# reference implementation for network compatibility.

use serde::{Deserialize, Serialize};

/// Neo N3 protocol settings
///
/// These parameters define the blockchain behavior and must be consistent
/// across all nodes in a network for consensus to work properly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSettings {
    /// Network identifier (magic number)
    pub network: u32,

    /// Address version byte for address encoding
    #[serde(default = "default_address_version")]
    pub address_version: u8,

    /// Milliseconds per block
    #[serde(default = "default_ms_per_block")]
    pub ms_per_block: u64,

    /// Maximum time for transaction validity (in blocks)
    #[serde(default = "default_max_valid_until_block_increment")]
    pub max_valid_until_block_increment: u32,

    /// Maximum number of validators
    #[serde(default = "default_validators_count")]
    pub validators_count: u32,

    /// Maximum number of transactions per block
    #[serde(default = "default_max_transactions_per_block")]
    pub max_transactions_per_block: u32,

    /// Memory pool capacity
    #[serde(default = "default_memory_pool_max_transactions")]
    pub memory_pool_max_transactions: u32,

    /// Initial GAS distribution amount
    #[serde(default = "default_initial_gas_distribution")]
    pub initial_gas_distribution: i64,

    /// Standby validators public keys (hex encoded)
    #[serde(default)]
    pub standby_validators: Vec<String>,

    /// Seed nodes
    #[serde(default)]
    pub seed_list: Vec<String>,

    /// Native contract activation heights
    #[serde(default)]
    pub native_activation_heights: NativeActivationHeights,

    /// Hardfork activation heights
    #[serde(default)]
    pub hardforks: HardforkHeights,
}

/// Native contract activation heights
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NativeActivationHeights {
    /// ContractManagement activation
    #[serde(default)]
    pub contract_management: u32,

    /// StdLib activation
    #[serde(default)]
    pub std_lib: u32,

    /// CryptoLib activation
    #[serde(default)]
    pub crypto_lib: u32,

    /// LedgerContract activation
    #[serde(default)]
    pub ledger: u32,

    /// NeoToken activation
    #[serde(default)]
    pub neo_token: u32,

    /// GasToken activation
    #[serde(default)]
    pub gas_token: u32,

    /// PolicyContract activation
    #[serde(default)]
    pub policy: u32,

    /// RoleManagement activation
    #[serde(default)]
    pub role_management: u32,

    /// OracleContract activation
    #[serde(default)]
    pub oracle: u32,
}

/// Hardfork activation heights
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardforkHeights {
    /// HF_Aspidochelone (pre-N3 GA)
    #[serde(default)]
    pub hf_aspidochelone: Option<u32>,

    /// HF_Basilisk
    #[serde(default)]
    pub hf_basilisk: Option<u32>,

    /// HF_Cockatrice
    #[serde(default)]
    pub hf_cockatrice: Option<u32>,

    /// HF_Domovoi
    #[serde(default)]
    pub hf_domovoi: Option<u32>,

    /// HF_Echidna
    #[serde(default)]
    pub hf_echidna: Option<u32>,
}

// Default value functions
fn default_address_version() -> u8 {
    0x35 // 'N' prefix for Neo addresses
}

fn default_ms_per_block() -> u64 {
    15000 // 15 seconds
}

fn default_max_valid_until_block_increment() -> u32 {
    5760 // ~24 hours at 15 sec/block
}

fn default_validators_count() -> u32 {
    7
}

fn default_max_transactions_per_block() -> u32 {
    512
}

fn default_memory_pool_max_transactions() -> u32 {
    50000
}

fn default_initial_gas_distribution() -> i64 {
    52_000_000_00000000 // 52 million GAS in datoshi
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl ProtocolSettings {
    /// Default settings (alias for mainnet, useful for testing)
    ///
    /// This provides a convenient default configuration that matches
    /// the Neo N3 MainNet settings. For production code, prefer using
    /// `mainnet()`, `testnet()`, or `private()` explicitly.
    #[inline]
    pub fn default_settings() -> Self {
        Self::mainnet()
    }

    /// MainNet protocol settings
    pub fn mainnet() -> Self {
        Self {
            network: 860833102,
            address_version: 0x35,
            ms_per_block: 15000,
            max_valid_until_block_increment: 5760,
            validators_count: 7,
            max_transactions_per_block: 512,
            memory_pool_max_transactions: 50000,
            initial_gas_distribution: 52_000_000_00000000,
            standby_validators: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a".to_string(),
                "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554".to_string(),
                "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d".to_string(),
                "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e".to_string(),
                "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70".to_string(),
            ],
            seed_list: vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
            ],
            native_activation_heights: NativeActivationHeights::default(),
            hardforks: HardforkHeights {
                hf_aspidochelone: Some(1730000),
                hf_basilisk: Some(4120000),
                hf_cockatrice: Some(5450000),
                hf_domovoi: None,
                hf_echidna: None,
            },
        }
    }

    /// TestNet protocol settings
    pub fn testnet() -> Self {
        Self {
            network: 894710606,
            address_version: 0x35,
            ms_per_block: 15000,
            max_valid_until_block_increment: 5760,
            validators_count: 7,
            max_transactions_per_block: 512,
            memory_pool_max_transactions: 50000,
            initial_gas_distribution: 52_000_000_00000000,
            standby_validators: vec![
                "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d".to_string(),
                "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2".to_string(),
                "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd".to_string(),
                "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806".to_string(),
                "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b".to_string(),
                "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01".to_string(),
                "030205e9cefaea5a1dfc580571e0b0123f3b4e55e1ceda5e6a0c7ecab2e01e7e01".to_string(),
            ],
            seed_list: vec![
                "seed1t5.neo.org:20333".to_string(),
                "seed2t5.neo.org:20333".to_string(),
                "seed3t5.neo.org:20333".to_string(),
                "seed4t5.neo.org:20333".to_string(),
                "seed5t5.neo.org:20333".to_string(),
            ],
            native_activation_heights: NativeActivationHeights::default(),
            hardforks: HardforkHeights {
                hf_aspidochelone: Some(210000),
                hf_basilisk: Some(2680000),
                hf_cockatrice: Some(3967000),
                hf_domovoi: None,
                hf_echidna: None,
            },
        }
    }

    /// Private network protocol settings
    pub fn private(network_magic: u32) -> Self {
        Self {
            network: network_magic,
            address_version: 0x35,
            ms_per_block: 1000, // Faster for testing
            max_valid_until_block_increment: 5760,
            validators_count: 1,
            max_transactions_per_block: 512,
            memory_pool_max_transactions: 50000,
            initial_gas_distribution: 52_000_000_00000000,
            standby_validators: vec![],
            seed_list: vec![],
            native_activation_heights: NativeActivationHeights::default(),
            hardforks: HardforkHeights::default(),
        }
    }

    /// Check if a hardfork is enabled at the given height
    pub fn is_hardfork_enabled(&self, hardfork: &str, height: u32) -> bool {
        match hardfork.to_lowercase().as_str() {
            "aspidochelone" | "hf_aspidochelone" => {
                self.hardforks.hf_aspidochelone.map_or(false, |h| height >= h)
            }
            "basilisk" | "hf_basilisk" => {
                self.hardforks.hf_basilisk.map_or(false, |h| height >= h)
            }
            "cockatrice" | "hf_cockatrice" => {
                self.hardforks.hf_cockatrice.map_or(false, |h| height >= h)
            }
            "domovoi" | "hf_domovoi" => {
                self.hardforks.hf_domovoi.map_or(false, |h| height >= h)
            }
            "echidna" | "hf_echidna" => {
                self.hardforks.hf_echidna.map_or(false, |h| height >= h)
            }
            _ => false,
        }
    }

    /// Get the number of committee members (21 for Neo N3)
    pub fn committee_count(&self) -> u32 {
        21.max(self.validators_count)
    }

    /// Calculate time span for a given number of blocks
    pub fn time_per_block(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.ms_per_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_settings() {
        let settings = ProtocolSettings::mainnet();
        assert_eq!(settings.network, 860833102);
        assert_eq!(settings.validators_count, 7);
        assert_eq!(settings.standby_validators.len(), 7);
    }

    #[test]
    fn test_hardfork_enabled() {
        let settings = ProtocolSettings::mainnet();
        assert!(settings.is_hardfork_enabled("aspidochelone", 1730001));
        assert!(!settings.is_hardfork_enabled("aspidochelone", 1729999));
        assert!(!settings.is_hardfork_enabled("echidna", 10000000));
    }

    #[test]
    fn test_committee_count() {
        let settings = ProtocolSettings::mainnet();
        assert_eq!(settings.committee_count(), 21);
    }
}

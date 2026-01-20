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

    /// Maximum number of blocks that can be traced by the VM
    #[serde(default = "default_max_traceable_blocks")]
    pub max_traceable_blocks: u32,

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

    /// HF_Faun
    #[serde(default)]
    pub hf_faun: Option<u32>,

    /// HF_Gorgon
    #[serde(default)]
    pub hf_gorgon: Option<u32>,
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

fn default_max_traceable_blocks() -> u32 {
    2_102_400
}

fn default_initial_gas_distribution() -> i64 {
    5_200_000_000_000_000 // 52 million GAS in datoshi
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
            max_traceable_blocks: 2_102_400,
            initial_gas_distribution: 5_200_000_000_000_000,
            standby_validators: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a".to_string(),
                "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554".to_string(),
                "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d".to_string(),
                "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e".to_string(),
                "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70".to_string(),
                "023a36c72844610b4d34d1968662424011bf783ca9d984efa19a20babf5582f3fe".to_string(),
                "03708b860c1de5d87f5b151a12c2a99feebd2e8b315ee8e7cf8aa19692a9e18379".to_string(),
                "03c6aa6e12638b36e88adc1ccdceac4db9929575c3e03576c617c49cce7114a050".to_string(),
                "03204223f8c86b8cd5c89ef12e4f0dbb314172e9241e30c9ef2293790793537cf0".to_string(),
                "02a62c915cf19c7f19a50ec217e79fac2439bbaad658493de0c7d8ffa92ab0aa62".to_string(),
                "03409f31f0d66bdc2f70a9730b66fe186658f84a8018204db01c106edc36553cd0".to_string(),
                "0288342b141c30dc8ffcde0204929bb46aed5756b41ef4a56778d15ada8f0c6654".to_string(),
                "020f2887f41474cfeb11fd262e982051c1541418137c02a0f4961af911045de639".to_string(),
                "0222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c30".to_string(),
                "03d281b42002647f0113f36c7b8efb30db66078dfaaa9ab3ff76d043a98d512fde".to_string(),
                "02504acbc1f4b3bdad1d86d6e1a08603771db135a73e61c9d565ae06a1938cd2ad".to_string(),
                "0226933336f1b75baa42d42b71d9091508b638046d19abd67f4e119bf64a7cfb4d".to_string(),
                "03cdcea66032b82f5c30450e381e5295cae85c5e6943af716cc6b646352a6067dc".to_string(),
                "02cd5a5547119e24feaa7c2a0f37b8c9366216bab7054de0065c9be42084003c8a".to_string(),
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
                hf_domovoi: Some(5570000),
                hf_echidna: Some(7300000),
                hf_faun: None,
                hf_gorgon: None,
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
            max_transactions_per_block: 5000,
            memory_pool_max_transactions: 50000,
            max_traceable_blocks: 2_102_400,
            initial_gas_distribution: 5_200_000_000_000_000,
            standby_validators: vec![
                "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d".to_string(),
                "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2".to_string(),
                "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd".to_string(),
                "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806".to_string(),
                "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b".to_string(),
                "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01".to_string(),
                "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba".to_string(),
                "025831cee3708e87d78211bec0d1bfee9f4c85ae784762f042e7f31c0d40c329b8".to_string(),
                "02cf9dc6e85d581480d91e88e8cbeaa0c153a046e89ded08b4cefd851e1d7325b5".to_string(),
                "03840415b0a0fcf066bcc3dc92d8349ebd33a6ab1402ef649bae00e5d9f5840828".to_string(),
                "026328aae34f149853430f526ecaa9cf9c8d78a4ea82d08bdf63dd03c4d0693be6".to_string(),
                "02c69a8d084ee7319cfecf5161ff257aa2d1f53e79bf6c6f164cff5d94675c38b3".to_string(),
                "0207da870cedb777fceff948641021714ec815110ca111ccc7a54c168e065bda70".to_string(),
                "035056669864feea401d8c31e447fb82dd29f342a9476cfd449584ce2a6165e4d7".to_string(),
                "0370c75c54445565df62cfe2e76fbec4ba00d1298867972213530cae6d418da636".to_string(),
                "03957af9e77282ae3263544b7b2458903624adc3f5dee303957cb6570524a5f254".to_string(),
                "03d84d22b8753cf225d263a3a782a4e16ca72ef323cfde04977c74f14873ab1e4c".to_string(),
                "02147c1b1d5728e1954958daff2f88ee2fa50a06890a8a9db3fa9e972b66ae559f".to_string(),
                "03c609bea5a4825908027e4ab217e7efc06e311f19ecad9d417089f14927a173d5".to_string(),
                "0231edee3978d46c335e851c76059166eb8878516f459e085c0dd092f0f1d51c21".to_string(),
                "03184b018d6b2bc093e535519732b3fd3f7551c8cffaf4621dd5a0b89482ca66c9".to_string(),
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
                hf_domovoi: Some(4144000),
                hf_echidna: Some(5870000),
                hf_faun: None,
                hf_gorgon: None,
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
            max_traceable_blocks: 2_102_400,
            initial_gas_distribution: 5_200_000_000_000_000,
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
                self.hardforks.hf_aspidochelone.is_some_and(|h| height >= h)
            }
            "basilisk" | "hf_basilisk" => self.hardforks.hf_basilisk.is_some_and(|h| height >= h),
            "cockatrice" | "hf_cockatrice" => {
                self.hardforks.hf_cockatrice.is_some_and(|h| height >= h)
            }
            "domovoi" | "hf_domovoi" => self.hardforks.hf_domovoi.is_some_and(|h| height >= h),
            "echidna" | "hf_echidna" => self.hardforks.hf_echidna.is_some_and(|h| height >= h),
            "faun" | "hf_faun" => self.hardforks.hf_faun.is_some_and(|h| height >= h),
            "gorgon" | "hf_gorgon" => self.hardforks.hf_gorgon.is_some_and(|h| height >= h),
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
        assert_eq!(settings.standby_validators.len(), 21);
    }

    #[test]
    fn test_hardfork_enabled() {
        let settings = ProtocolSettings::mainnet();
        assert!(settings.is_hardfork_enabled("aspidochelone", 1730001));
        assert!(!settings.is_hardfork_enabled("aspidochelone", 1729999));
        assert!(settings.is_hardfork_enabled("echidna", 7300000));
        assert!(!settings.is_hardfork_enabled("echidna", 7299999));
    }

    #[test]
    fn test_committee_count() {
        let settings = ProtocolSettings::mainnet();
        assert_eq!(settings.committee_count(), 21);
    }
}

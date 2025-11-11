use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::Deserialize;

use crate::settings::{
    error::ProtocolSettingsError,
    protocol::{ProtocolSettings, ProtocolSettingsOverrides},
    scrypt::ScryptSettingsConfig,
};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ProtocolSettingsConfig {
    pub network: Option<u32>,
    pub address_version: Option<u8>,
    pub milliseconds_per_block: Option<u32>,
    pub max_transactions_per_block: Option<u32>,
    pub memory_pool_max_transactions: Option<u32>,
    pub max_traceable_blocks: Option<u32>,
    pub max_valid_until_block_increment: Option<u32>,
    pub validators_count: Option<usize>,
    pub standby_committee: Option<Vec<String>>,
    pub seed_list: Option<Vec<String>>,
    pub initial_gas_distribution: Option<u64>,
    pub scrypt: Option<ScryptSettingsConfig>,
    pub hardforks: Option<BTreeMap<String, u32>>,
}

impl ProtocolSettingsConfig {
    pub fn apply_to(
        self,
        base: ProtocolSettings,
    ) -> Result<ProtocolSettings, ProtocolSettingsError> {
        let overrides = ProtocolSettingsOverrides::try_from(self)?;
        base.with_overrides(overrides)
    }
}

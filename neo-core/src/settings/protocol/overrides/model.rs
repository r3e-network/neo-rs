use alloc::{collections::BTreeMap, vec::Vec};

use neo_crypto::ecc256::PublicKey;

use crate::settings::{error::ProtocolSettingsError, hardfork::Hardfork, scrypt::ScryptSettings};

use super::super::settings::ProtocolSettings;

#[derive(Debug, Clone, Default)]
pub struct ProtocolSettingsOverrides {
    pub network_magic: Option<u32>,
    pub address_version: Option<u8>,
    pub milliseconds_per_block: Option<u32>,
    pub max_valid_until_block_increment: Option<u32>,
    pub max_transactions_per_block: Option<u32>,
    pub memory_pool_max_transactions: Option<u32>,
    pub max_traceable_blocks: Option<u32>,
    pub validators_count: Option<usize>,
    pub standby_committee: Option<Vec<PublicKey>>,
    pub seed_list: Option<Vec<String>>,
    pub scrypt: Option<ScryptSettings>,
    pub initial_gas_distribution: Option<u64>,
    pub hardforks: Option<BTreeMap<Hardfork, u32>>,
}

impl ProtocolSettingsOverrides {
    pub fn apply(self, base: ProtocolSettings) -> Result<ProtocolSettings, ProtocolSettingsError> {
        base.with_overrides(self)
    }
}

use alloc::{string::ToString, vec::Vec};

use crate::settings::config::ProtocolSettingsConfig;

use super::super::model::ProtocolSettingsOverrides;

pub(super) fn apply_numeric_overrides(
    config: &ProtocolSettingsConfig,
    overrides: &mut ProtocolSettingsOverrides,
) {
    overrides.network_magic = config.network;
    overrides.address_version = config.address_version;
    overrides.milliseconds_per_block = config.milliseconds_per_block;
    overrides.max_transactions_per_block = config.max_transactions_per_block;
    overrides.memory_pool_max_transactions = config.memory_pool_max_transactions;
    overrides.max_traceable_blocks = config.max_traceable_blocks;
    overrides.max_valid_until_block_increment = config.max_valid_until_block_increment;
    overrides.validators_count = config.validators_count;
}

pub(super) fn normalize_seed_list(seeds: Option<Vec<String>>) -> Option<Vec<String>> {
    seeds.map(|entries| {
        entries
            .into_iter()
            .map(|entry| entry.trim().to_string())
            .collect()
    })
}

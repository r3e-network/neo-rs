use neo_crypto::ecc256::PublicKey;

use super::super::{calculate_max_valid_until_block_increment, settings::ProtocolSettings};
use super::networks::NetworkDefaults;
use crate::settings::{hardfork, scrypt::ScryptSettings};

pub(crate) fn build_settings(defaults: &NetworkDefaults) -> ProtocolSettings {
    ProtocolSettings {
        network_magic: defaults.magic,
        address_version: defaults.address_version,
        milliseconds_per_block: defaults.milliseconds_per_block,
        max_valid_until_block_increment: calculate_max_valid_until_block_increment(
            defaults.milliseconds_per_block,
        ),
        max_transactions_per_block: defaults.max_transactions_per_block,
        memory_pool_max_transactions: defaults.memory_pool_max_transactions,
        max_traceable_blocks: defaults.max_traceable_blocks,
        validators_count: defaults.validators_count,
        standby_committee: parse_committee(defaults.standby_committee),
        seed_list: defaults.seed_list.iter().map(|s| s.to_string()).collect(),
        scrypt: ScryptSettings::default(),
        initial_gas_distribution: defaults.initial_gas_distribution,
        hardforks: hardfork::build_hardfork_map(defaults.hardforks),
    }
}

fn parse_committee(entries: &[&str]) -> Vec<PublicKey> {
    entries
        .iter()
        .map(|hex| {
            let bytes = hex::decode(hex).expect("committee hex");
            PublicKey::from_sec1_bytes(&bytes).expect("committee key")
        })
        .collect()
}

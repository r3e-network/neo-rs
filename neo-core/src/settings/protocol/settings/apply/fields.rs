use alloc::vec::Vec;

use crate::settings::{scrypt::ScryptSettings, ProtocolSettings};

pub(super) fn apply_simple_fields(
    settings: &mut ProtocolSettings,
    network_magic: Option<u32>,
    address_version: Option<u8>,
    seed_list: Option<Vec<String>>,
    milliseconds_per_block: Option<u32>,
    max_transactions_per_block: Option<u32>,
    memory_pool_max_transactions: Option<u32>,
    max_traceable_blocks: Option<u32>,
    max_valid_until_block_increment: Option<u32>,
    scrypt_override: Option<ScryptSettings>,
    initial_gas_distribution: Option<u64>,
    calc_max_valid: fn(u32) -> u32,
) {
    if let Some(magic) = network_magic {
        settings.network_magic = magic;
    }
    if let Some(version) = address_version {
        settings.address_version = version;
    }
    if let Some(seeds) = seed_list {
        settings.seed_list = seeds;
    }
    if let Some(milliseconds) = milliseconds_per_block {
        settings.milliseconds_per_block = milliseconds;
    }
    settings.max_valid_until_block_increment = max_valid_until_block_increment
        .unwrap_or_else(|| calc_max_valid(settings.milliseconds_per_block));
    if let Some(max_tx) = max_transactions_per_block {
        settings.max_transactions_per_block = max_tx;
    }
    if let Some(mempool_limit) = memory_pool_max_transactions {
        settings.memory_pool_max_transactions = mempool_limit;
    }
    if let Some(max_traceable) = max_traceable_blocks {
        settings.max_traceable_blocks = max_traceable;
    }
    if let Some(scrypt) = scrypt_override {
        settings.scrypt = scrypt;
    }
    if let Some(initial_gas) = initial_gas_distribution {
        settings.initial_gas_distribution = initial_gas;
    }
}

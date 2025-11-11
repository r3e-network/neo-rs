use alloc::{collections::BTreeMap, string::String, vec::Vec};

use neo_crypto::ecc256::PublicKey;

use crate::settings::{hardfork::Hardfork, scrypt::ScryptSettings};

#[derive(Clone, Debug)]
pub struct ProtocolSettings {
    pub network_magic: u32,
    pub address_version: u8,
    pub milliseconds_per_block: u32,
    pub max_valid_until_block_increment: u32,
    pub max_transactions_per_block: u32,
    pub memory_pool_max_transactions: u32,
    pub max_traceable_blocks: u32,
    pub validators_count: usize,
    pub standby_committee: Vec<PublicKey>,
    pub seed_list: Vec<String>,
    pub scrypt: ScryptSettings,
    pub initial_gas_distribution: u64,
    pub hardforks: BTreeMap<Hardfork, u32>,
}

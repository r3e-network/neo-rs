use crate::settings::hardfork::Hardfork;

use super::NetworkDefaults;

pub(crate) const PRIVATENET: NetworkDefaults = NetworkDefaults {
    magic: 1_515_151,
    address_version: 0x35,
    milliseconds_per_block: 15_000,
    max_transactions_per_block: 512,
    memory_pool_max_transactions: 50_000,
    max_traceable_blocks: 65_536,
    validators_count: 1,
    standby_committee: &["0222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c30"],
    seed_list: &["127.0.0.1:20333"],
    initial_gas_distribution: 5_200_000_000_000_000,
    hardforks: &[(Hardfork::Aspidochelone, 0)],
};

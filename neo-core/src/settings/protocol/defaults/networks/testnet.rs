use crate::settings::hardfork::Hardfork;

use super::NetworkDefaults;

pub(crate) const TESTNET: NetworkDefaults = NetworkDefaults {
    magic: 894_710_606,
    address_version: 0x35,
    milliseconds_per_block: 15_000,
    max_transactions_per_block: 512,
    memory_pool_max_transactions: 50_000,
    max_traceable_blocks: 2_102_400,
    validators_count: 7,
    standby_committee: super::MAINNET.standby_committee,
    seed_list: &["seed1t.neo.org:20333", "seed2t.neo.org:20333"],
    initial_gas_distribution: 5_200_000_000_000_000,
    hardforks: &[
        (Hardfork::Aspidochelone, 0),
        (Hardfork::Basilisk, 2_680_000),
    ],
};

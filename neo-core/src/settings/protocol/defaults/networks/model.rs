use crate::settings::hardfork::Hardfork;

pub(crate) struct NetworkDefaults {
    pub(crate) magic: u32,
    pub(crate) address_version: u8,
    pub(crate) milliseconds_per_block: u32,
    pub(crate) max_transactions_per_block: u32,
    pub(crate) memory_pool_max_transactions: u32,
    pub(crate) max_traceable_blocks: u32,
    pub(crate) validators_count: usize,
    pub(crate) standby_committee: &'static [&'static str],
    pub(crate) seed_list: &'static [&'static str],
    pub(crate) initial_gas_distribution: u64,
    pub(crate) hardforks: &'static [(Hardfork, u32)],
}

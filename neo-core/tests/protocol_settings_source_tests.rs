use neo_core::hardfork::Hardfork;
use neo_core::protocol_settings::ProtocolSettings;

#[test]
fn mainnet_runtime_settings_match_shared_config_source() {
    let core = ProtocolSettings::mainnet();
    let config = neo_config::ProtocolSettings::mainnet();

    assert_eq!(core.network, config.network);
    assert_eq!(core.address_version, config.address_version);
    assert_eq!(core.milliseconds_per_block as u64, config.ms_per_block);
    assert_eq!(
        core.max_valid_until_block_increment,
        config.max_valid_until_block_increment
    );
    assert_eq!(
        core.max_transactions_per_block,
        config.max_transactions_per_block
    );
    assert_eq!(
        core.memory_pool_max_transactions as u32,
        config.memory_pool_max_transactions
    );
    assert_eq!(core.max_traceable_blocks, config.max_traceable_blocks);
    assert_eq!(
        core.initial_gas_distribution as i64,
        config.initial_gas_distribution
    );
    assert_eq!(core.seed_list, config.seed_list);
    assert_eq!(
        core.standby_committee.len(),
        config.standby_validators.len()
    );
    assert_eq!(core.hardforks.get(&Hardfork::HfFaun), Some(&8_800_000));
    assert_eq!(core.hardforks.get(&Hardfork::HfGorgon), Some(&12_020_000));
    assert!(!core.hardforks.contains_key(&Hardfork::HfHuyao));
}

#[test]
fn testnet_runtime_settings_match_shared_config_source() {
    let core = ProtocolSettings::testnet();
    let config = neo_config::ProtocolSettings::testnet();

    assert_eq!(core.network, config.network);
    assert_eq!(core.address_version, config.address_version);
    assert_eq!(core.milliseconds_per_block as u64, config.ms_per_block);
    assert_eq!(
        core.max_valid_until_block_increment,
        config.max_valid_until_block_increment
    );
    assert_eq!(
        core.max_transactions_per_block,
        config.max_transactions_per_block
    );
    assert_eq!(
        core.memory_pool_max_transactions as u32,
        config.memory_pool_max_transactions
    );
    assert_eq!(core.max_traceable_blocks, config.max_traceable_blocks);
    assert_eq!(
        core.initial_gas_distribution as i64,
        config.initial_gas_distribution
    );
    assert_eq!(core.seed_list, config.seed_list);
    assert_eq!(
        core.standby_committee.len(),
        config.standby_validators.len()
    );
    assert_eq!(core.hardforks.get(&Hardfork::HfFaun), Some(&12_960_000));
    assert_eq!(core.hardforks.get(&Hardfork::HfGorgon), Some(&17_960_000));
    assert!(!core.hardforks.contains_key(&Hardfork::HfHuyao));
}

#[test]
fn default_runtime_settings_match_csharp_default() {
    let settings = ProtocolSettings::default();
    assert_eq!(settings.network, 0);
    assert!(settings.standby_committee.is_empty());
    assert!(!settings.hardforks.is_empty());
    assert!(settings.is_hardfork_enabled(Hardfork::HfEchidna, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfGorgon, 0));
}

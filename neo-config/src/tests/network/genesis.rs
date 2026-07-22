use super::*;

#[test]
fn test_mainnet_genesis() {
    let genesis = GenesisConfig::mainnet();
    assert_eq!(genesis.validators.len(), 7);
    assert!(genesis.validate().is_ok());
}

#[test]
fn test_testnet_genesis() {
    let genesis = GenesisConfig::testnet();
    assert_eq!(genesis.validators.len(), 7);
    assert!(genesis.validate().is_ok());
}

#[test]
fn test_private_genesis() {
    let genesis = GenesisConfig::private(
        "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        GENESIS_TIMESTAMP_MS,
        GENESIS_NONCE,
    );
    assert_eq!(genesis.validators.len(), 1);
    assert!(genesis.validate().is_ok());
}

#[test]
fn test_invalid_genesis() {
    let genesis = GenesisConfig {
        timestamp: 0,
        nonce: 0,
        validators: vec![],
        committee: vec![],
        distribution: vec![],
        contracts: vec![],
    };
    assert!(genesis.validate().is_err());
}

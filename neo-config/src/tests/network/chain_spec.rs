use super::*;

#[test]
fn mainnet_spec_pins_identity_genesis_and_hardforks() {
    let spec = NeoChainSpec::mainnet().expect("valid embedded MainNet spec");

    assert_eq!(spec.identity().name(), "neo-n3-mainnet");
    assert_eq!(spec.identity().network_type(), NetworkType::MainNet);
    assert_eq!(
        Some(spec.network_magic()),
        NetworkType::MainNet.canonical_magic()
    );
    assert_eq!(spec.genesis().validators.len(), 7);
    assert_eq!(
        spec.identity()
            .expected_genesis_hash()
            .expect("MainNet hash")
            .to_string(),
        MAINNET_GENESIS_HASH
    );
    assert_eq!(
        spec.hardfork_activation(Hardfork::HfGorgon),
        Some(12_020_000)
    );
    assert!(!spec.hardforks().is_defined(Hardfork::HfHuyao));
}

#[test]
fn testnet_spec_is_explicit_and_validated() {
    let spec = NeoChainSpec::testnet().expect("valid embedded TestNet spec");

    assert_eq!(spec.identity().network_type(), NetworkType::TestNet);
    assert_eq!(
        Some(spec.network_magic()),
        NetworkType::TestNet.canonical_magic()
    );
    assert_eq!(spec.hardfork_activation(Hardfork::HfFaun), Some(12_960_000));
    assert_eq!(
        spec.identity()
            .expected_genesis_hash()
            .expect("TestNet hash")
            .to_string(),
        TESTNET_GENESIS_HASH
    );
}

#[test]
fn built_in_genesis_pins_reject_a_different_hash() {
    for spec in [
        NeoChainSpec::mainnet().expect("valid embedded MainNet spec"),
        NeoChainSpec::testnet().expect("valid embedded TestNet spec"),
    ] {
        let error = spec
            .validate_genesis_hash(UInt256::zero())
            .expect_err("public networks must fail closed on a different genesis hash");

        assert!(matches!(error, ChainSpecError::GenesisHashMismatch { .. }));
    }
}

#[test]
fn private_spec_has_closed_private_identity() {
    let spec = NeoChainSpec::private(
        "private-chain",
        ProtocolSettings::mainnet(),
        GenesisConfig::mainnet(),
        None,
    )
    .expect("valid private chain spec");

    assert_eq!(spec.identity().network_type(), NetworkType::Private);
}

#[test]
fn committee_mismatch_is_rejected_at_the_chain_boundary() {
    let protocol = ProtocolSettings::mainnet();
    let mut genesis = GenesisConfig::mainnet();
    genesis.committee.swap(0, 1);

    let error = NeoChainSpec::private("invalid-committee", protocol, genesis, None)
        .expect_err("mismatched committee must fail");

    assert!(matches!(
        error,
        ChainSpecError::GenesisCommitteeMismatch { index: 0 }
    ));
}

#[test]
fn chain_spec_provider_preserves_arc_identity() {
    let spec = NeoChainSpec::mainnet().expect("valid embedded MainNet spec");
    let provided = ChainSpecProvider::chain_spec(&spec);

    assert!(Arc::ptr_eq(&spec, &provided));
}

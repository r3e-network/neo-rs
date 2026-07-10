use super::*;

fn supported_standards<C>(contract: &C, settings: &ProtocolSettings, height: u32) -> Vec<String>
where
    C: NativeContract,
{
    build_native_contract_state(contract, settings, height)
        .manifest
        .supported_standards
}

fn assert_initialize_block<C>(
    contract: &C,
    settings: &ProtocolSettings,
    height: u32,
    expected_initialize: bool,
    expected_hits: &[Hardfork],
) where
    C: NativeContract,
{
    let (is_initialize, hits) = contract.is_initialize_block(settings, height);
    assert_eq!(
        is_initialize,
        expected_initialize,
        "{} initialize at height {height}",
        contract.name()
    );
    assert_eq!(
        hits,
        expected_hits,
        "{} hits at height {height}",
        contract.name()
    );
}

fn assert_activation_spec<C>(
    contract: &C,
    active_in: Option<Hardfork>,
    csharp_activations: &[Option<Hardfork>],
) where
    C: NativeContract,
{
    let spec = standard_native_contract_specs()
        .into_iter()
        .find(|spec| spec.name == contract.name())
        .expect("standard native contract spec");

    assert_eq!(contract.active_in(), active_in, "{}", contract.name());
    assert_eq!(
        spec.csharp_activations,
        csharp_activations,
        "{} C# activation schedule",
        contract.name()
    );
    assert_eq!(
        spec.csharp_activations.first().copied().flatten(),
        contract.active_in(),
        "{} ActiveIn must match first C# activation entry",
        contract.name()
    );
}

#[test]
fn csharp_activation_schedules_drive_runtime_activation_and_manifest_refresh() {
    let settings = test_settings();
    assert_activation_spec(&OracleContract, None, &[None, Some(Hardfork::HfFaun)]);
    assert_activation_spec(
        &Notary,
        Some(Hardfork::HfEchidna),
        &[Some(Hardfork::HfEchidna), Some(Hardfork::HfFaun)],
    );
    assert_activation_spec(&Treasury, Some(Hardfork::HfFaun), &[Some(Hardfork::HfFaun)]);

    let oracle = OracleContract;
    assert!(oracle.is_active(&settings, GENESIS));
    assert!(oracle.contract_state(&settings, GENESIS).is_some());
    assert_initialize_block(&oracle, &settings, GENESIS, true, &[]);
    assert_initialize_block(&oracle, &settings, 59, false, &[]);
    assert_initialize_block(&oracle, &settings, 60, true, &[Hardfork::HfFaun]);
    assert!(supported_standards(&oracle, &settings, 59).is_empty());
    assert_eq!(supported_standards(&oracle, &settings, 60), ["NEP-30"]);

    let notary = Notary;
    assert!(!notary.is_active(&settings, 49));
    assert!(notary.contract_state(&settings, 49).is_none());
    assert!(notary.is_active(&settings, 50));
    assert!(notary.contract_state(&settings, 50).is_some());
    assert_initialize_block(&notary, &settings, 49, false, &[]);
    assert_initialize_block(&notary, &settings, 50, true, &[Hardfork::HfEchidna]);
    assert_initialize_block(&notary, &settings, 60, true, &[Hardfork::HfFaun]);
    assert_eq!(supported_standards(&notary, &settings, 50), ["NEP-27"]);
    assert_eq!(
        supported_standards(&notary, &settings, 60),
        ["NEP-27", "NEP-30"]
    );

    let treasury = Treasury;
    assert!(!treasury.is_active(&settings, 59));
    assert!(treasury.contract_state(&settings, 59).is_none());
    assert!(treasury.is_active(&settings, 60));
    assert!(treasury.contract_state(&settings, 60).is_some());
    assert_initialize_block(&treasury, &settings, 59, false, &[]);
    assert_initialize_block(&treasury, &settings, 60, true, &[Hardfork::HfFaun]);
    assert_eq!(
        supported_standards(&treasury, &settings, 60),
        ["NEP-26", "NEP-27", "NEP-30"]
    );
}

#[test]
fn omitted_active_in_hardfork_matches_csharp_is_active_asymmetry() {
    let mut settings = test_settings();
    settings.hardforks.remove(&Hardfork::HfEchidna);
    settings.hardforks.remove(&Hardfork::HfFaun);

    fn assert_genesis_active_without_config<C>(contract: &C, settings: &ProtocolSettings)
    where
        C: NativeContract,
    {
        assert!(
            contract.is_active(settings, GENESIS),
            "{} should be genesis-active when ActiveIn is not configured",
            contract.name()
        );
        assert!(
            contract.contract_state(settings, GENESIS).is_some(),
            "{} should expose contract state under C# IsActive fallback",
            contract.name()
        );
        assert_initialize_block(contract, settings, GENESIS, false, &[]);
    }

    assert_genesis_active_without_config(&Notary, &settings);
    assert_genesis_active_without_config(&Treasury, &settings);

    let oracle = OracleContract;
    assert!(oracle.is_active(&settings, GENESIS));
    assert_initialize_block(&oracle, &settings, GENESIS, true, &[]);
    assert!(supported_standards(&oracle, &settings, GENESIS).is_empty());
}

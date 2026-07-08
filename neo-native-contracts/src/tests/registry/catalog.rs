use super::*;

use neo_config::Hardfork;

#[test]
fn descriptors_match_constructed_contract_handles() {
    for descriptor in standard_native_contract_descriptors() {
        let contract = descriptor.contract();
        let spec = descriptor.spec();
        assert_eq!(contract.id(), descriptor.id, "{} id", descriptor.name);
        assert_eq!(contract.name(), descriptor.name, "{} name", descriptor.name);
        assert_eq!(
            contract.hash(),
            (descriptor.hash)(),
            "{} script hash",
            descriptor.name
        );
        assert_eq!(
            contract.active_in(),
            spec.active_in,
            "{} ActiveIn",
            descriptor.name
        );
        assert_eq!(
            contract.activations(),
            spec.activations,
            "{} activations",
            descriptor.name
        );
    }
}

#[test]
fn standard_contract_handles_share_a_uniform_shape() {
    fn assert_handle<T>()
    where
        T: NativeContract + Default + Copy + Clone + std::fmt::Debug + 'static,
    {
    }

    assert_handle::<ContractManagement>();
    assert_handle::<StdLib>();
    assert_handle::<CryptoLib>();
    assert_handle::<LedgerContract>();
    assert_handle::<NeoToken>();
    assert_handle::<GasToken>();
    assert_handle::<PolicyContract>();
    assert_handle::<RoleManagement>();
    assert_handle::<OracleContract>();
    assert_handle::<Notary>();
    assert_handle::<Treasury>();
}

#[test]
fn standard_contract_handles_are_const_constructible() {
    const CONTRACT_MANAGEMENT: ContractManagement = ContractManagement::new();
    const STD_LIB: StdLib = StdLib::new();
    const CRYPTO_LIB: CryptoLib = CryptoLib::new();
    const LEDGER_CONTRACT: LedgerContract = LedgerContract::new();
    const NEO_TOKEN: NeoToken = NeoToken::new();
    const GAS_TOKEN: GasToken = GasToken::new();
    const POLICY_CONTRACT: PolicyContract = PolicyContract::new();
    const ROLE_MANAGEMENT: RoleManagement = RoleManagement::new();
    const ORACLE_CONTRACT: OracleContract = OracleContract::new();
    const NOTARY: Notary = Notary::new();
    const TREASURY: Treasury = Treasury::new();

    assert_eq!(CONTRACT_MANAGEMENT.name(), "ContractManagement");
    assert_eq!(STD_LIB.name(), "StdLib");
    assert_eq!(CRYPTO_LIB.name(), "CryptoLib");
    assert_eq!(LEDGER_CONTRACT.name(), "LedgerContract");
    assert_eq!(NEO_TOKEN.name(), "NeoToken");
    assert_eq!(GAS_TOKEN.name(), "GasToken");
    assert_eq!(POLICY_CONTRACT.name(), "PolicyContract");
    assert_eq!(ROLE_MANAGEMENT.name(), "RoleManagement");
    assert_eq!(ORACLE_CONTRACT.name(), "OracleContract");
    assert_eq!(NOTARY.name(), "Notary");
    assert_eq!(TREASURY.name(), "Treasury");
}

#[test]
fn standard_contract_activation_policy_matches_neo_n3_v3101() {
    let contracts = standard_native_contracts();
    let contract = |name: &str| {
        contracts
            .iter()
            .find(|contract| contract.name() == name)
            .unwrap_or_else(|| panic!("{name} should be in the standard native catalog"))
    };

    for native in &contracts {
        match native.name() {
            "Notary" => assert_eq!(native.active_in(), Some(Hardfork::HfEchidna)),
            "Treasury" => assert_eq!(native.active_in(), Some(Hardfork::HfFaun)),
            name => assert_eq!(
                native.active_in(),
                None,
                "{name} should be genesis-active in Neo N3 v3.10.1"
            ),
        }
    }

    assert!(
        contract("NeoToken")
            .used_hardforks()
            .contains(&Hardfork::HfEchidna),
        "NeoToken should refresh at Echidna through method metadata"
    );
    assert_eq!(contract("NeoToken").activations(), &[]);
    assert_eq!(
        contract("OracleContract").activations(),
        &[Hardfork::HfFaun]
    );
    assert_eq!(
        contract("Notary").activations(),
        &[Hardfork::HfEchidna, Hardfork::HfFaun]
    );
    assert_eq!(contract("Treasury").activations(), &[Hardfork::HfFaun]);

    for native in &contracts {
        match native.name() {
            "OracleContract" | "Notary" | "Treasury" => {}
            name => assert!(
                native.activations().is_empty(),
                "{name} should not duplicate refresh hardforks already present in method/event metadata"
            ),
        }
    }
}

#[test]
fn standard_catalog_pins_csharp_activation_schedules() {
    let schedules =
        standard_native_contract_specs().map(|spec| (spec.name, spec.csharp_activations));
    assert_eq!(
        schedules,
        [
            ("ContractManagement", &[][..]),
            ("StdLib", &[][..]),
            ("CryptoLib", &[][..]),
            ("LedgerContract", &[][..]),
            ("NeoToken", &[][..]),
            ("GasToken", &[][..]),
            ("PolicyContract", &[][..]),
            ("RoleManagement", &[][..]),
            ("OracleContract", &[None, Some(Hardfork::HfFaun)][..]),
            (
                "Notary",
                &[Some(Hardfork::HfEchidna), Some(Hardfork::HfFaun)][..],
            ),
            ("Treasury", &[Some(Hardfork::HfFaun)][..]),
        ]
    );
}

#[test]
fn standard_catalog_derives_activation_policy_from_contract_handles() {
    let source = include_str!("../../registry/catalog.rs");
    let descriptor_list = source
        .split("fn standard_native_contract_descriptors")
        .nth(1)
        .expect("catalog descriptor list");

    assert!(
        !descriptor_list.contains("active_in:"),
        "catalog descriptors should not duplicate NativeContract::active_in"
    );
    assert!(
        !descriptor_list.contains("activations:"),
        "catalog descriptors should not duplicate NativeContract::activations"
    );
}

#[test]
fn specs_keep_canonical_native_contract_order() {
    use crate::hashes::{
        CONTRACT_MANAGEMENT_HASH, CRYPTO_LIB_HASH, GAS_TOKEN_HASH, LEDGER_CONTRACT_HASH,
        NEO_TOKEN_HASH, NOTARY_HASH, ORACLE_CONTRACT_HASH, POLICY_CONTRACT_HASH,
        ROLE_MANAGEMENT_HASH, STDLIB_HASH, TREASURY_HASH,
    };

    let specs = standard_native_contract_specs();
    assert_eq!(specs.len(), STANDARD_NATIVE_CONTRACT_COUNT);
    assert_eq!(
        specs.map(|spec| {
            (
                spec.name,
                spec.id,
                spec.hash,
                spec.active_in,
                spec.activations,
                spec.csharp_activations,
            )
        }),
        [
            (
                "ContractManagement",
                -1,
                *CONTRACT_MANAGEMENT_HASH,
                None,
                &[][..],
                &[][..],
            ),
            ("StdLib", -2, *STDLIB_HASH, None, &[][..], &[][..]),
            ("CryptoLib", -3, *CRYPTO_LIB_HASH, None, &[][..], &[][..]),
            (
                "LedgerContract",
                -4,
                *LEDGER_CONTRACT_HASH,
                None,
                &[][..],
                &[][..],
            ),
            ("NeoToken", -5, *NEO_TOKEN_HASH, None, &[][..], &[][..],),
            ("GasToken", -6, *GAS_TOKEN_HASH, None, &[][..], &[][..]),
            (
                "PolicyContract",
                -7,
                *POLICY_CONTRACT_HASH,
                None,
                &[][..],
                &[][..],
            ),
            (
                "RoleManagement",
                -8,
                *ROLE_MANAGEMENT_HASH,
                None,
                &[][..],
                &[][..],
            ),
            (
                "OracleContract",
                -9,
                *ORACLE_CONTRACT_HASH,
                None,
                &[Hardfork::HfFaun][..],
                &[None, Some(Hardfork::HfFaun)][..],
            ),
            (
                "Notary",
                -10,
                *NOTARY_HASH,
                Some(Hardfork::HfEchidna),
                &[Hardfork::HfEchidna, Hardfork::HfFaun][..],
                &[Some(Hardfork::HfEchidna), Some(Hardfork::HfFaun)][..],
            ),
            (
                "Treasury",
                -11,
                *TREASURY_HASH,
                Some(Hardfork::HfFaun),
                &[Hardfork::HfFaun][..],
                &[Some(Hardfork::HfFaun)][..],
            ),
        ]
    );
}

#[test]
fn specs_have_unique_protocol_identifiers() {
    let specs = standard_native_contract_specs();
    for i in 0..specs.len() {
        for j in (i + 1)..specs.len() {
            assert_ne!(
                specs[i].id, specs[j].id,
                "duplicate native contract id for {} / {}",
                specs[i].name, specs[j].name
            );
            assert_ne!(
                specs[i].name, specs[j].name,
                "duplicate native contract name for id {} / {}",
                specs[i].id, specs[j].id
            );
            assert_ne!(
                specs[i].hash, specs[j].hash,
                "duplicate native contract hash for {} / {}",
                specs[i].name, specs[j].name
            );
        }
    }
}

#[test]
fn standard_hash_membership_matches_spec_catalog() {
    let specs = standard_native_contract_specs();
    assert_eq!(
        standard_native_contract_hashes(),
        specs.map(|spec| spec.hash)
    );
    for spec in specs {
        assert!(
            is_standard_native_contract_hash(&spec.hash),
            "{} hash should be recognized as standard",
            spec.name
        );
    }

    assert!(
        !is_standard_native_contract_hash(&UInt160::zero()),
        "zero hash must not be recognized as a standard native contract"
    );
}

#[test]
fn spec_lookup_matches_canonical_catalog() {
    for spec in standard_native_contract_specs() {
        assert_eq!(
            standard_native_contract_spec_by_id(spec.id),
            Some(spec),
            "{} id lookup",
            spec.name
        );
        assert_eq!(
            standard_native_contract_spec_by_hash(&spec.hash),
            Some(spec),
            "{} hash lookup",
            spec.name
        );
        assert_eq!(
            standard_native_contract_spec_by_name(spec.name),
            Some(spec),
            "{} exact name lookup",
            spec.name
        );
        assert_eq!(
            standard_native_contract_spec_by_name(&spec.name.to_ascii_lowercase()),
            Some(spec),
            "{} case-insensitive name lookup",
            spec.name
        );
    }

    assert_eq!(standard_native_contract_spec_by_id(0), None);
    assert_eq!(
        standard_native_contract_spec_by_hash(&UInt160::zero()),
        None
    );
    assert_eq!(standard_native_contract_spec_by_name("MissingNative"), None);
}

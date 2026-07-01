use super::*;

#[test]
fn native_contract_handle_names_are_uniform_constants() {
    let contracts: Vec<(&'static str, Box<dyn NativeContract>)> = vec![
        (
            ContractManagement::NAME,
            Box::new(ContractManagement::new()),
        ),
        (StdLib::NAME, Box::new(StdLib::new())),
        (CryptoLib::NAME, Box::new(CryptoLib::new())),
        (LedgerContract::NAME, Box::new(LedgerContract::new())),
        (NeoToken::NAME, Box::new(NeoToken::new())),
        (GasToken::NAME, Box::new(GasToken::new())),
        (PolicyContract::NAME, Box::new(PolicyContract::new())),
        (RoleManagement::NAME, Box::new(RoleManagement::new())),
        (OracleContract::NAME, Box::new(OracleContract::new())),
        (Notary::NAME, Box::new(Notary::new())),
        (Treasury::NAME, Box::new(Treasury::new())),
    ];

    assert_eq!(
        contracts.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec![
            "ContractManagement",
            "StdLib",
            "CryptoLib",
            "LedgerContract",
            "NeoToken",
            "GasToken",
            "PolicyContract",
            "RoleManagement",
            "OracleContract",
            "Notary",
            "Treasury",
        ]
    );

    for (name, contract) in contracts {
        assert_eq!(contract.name(), name);
    }
}

mod native_handle_api {
    use neo_native_contracts::{
        ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
        PolicyContract, RoleManagement, StdLib, Treasury,
    };
    use neo_primitives::UInt160;

    #[test]
    fn native_contract_handle_hash_accessors_are_uniform() {
        assert_eq!(
            ContractManagement::new().hash(),
            ContractManagement::script_hash()
        );
        assert_eq!(StdLib::new().hash(), StdLib::script_hash());
        assert_eq!(CryptoLib::new().hash(), CryptoLib::script_hash());
        assert_eq!(LedgerContract::new().hash(), LedgerContract::script_hash());
        assert_eq!(NeoToken::new().hash(), NeoToken::script_hash());
        assert_eq!(GasToken::new().hash(), GasToken::script_hash());
        assert_eq!(PolicyContract::new().hash(), PolicyContract::script_hash());
        assert_eq!(RoleManagement::new().hash(), RoleManagement::script_hash());
        assert_eq!(OracleContract::new().hash(), OracleContract::script_hash());
        assert_eq!(Notary::new().hash(), Notary::script_hash());
        assert_eq!(Treasury::new().hash(), Treasury::script_hash());
    }

    #[test]
    fn native_contract_hashes_match_csharp_v3_10_fixture() {
        let expected = [
            (
                ContractManagement::NAME,
                ContractManagement::script_hash(),
                "0xfffdc93764dbaddd97c48f252a53ea4643faa3fd",
            ),
            (
                StdLib::NAME,
                StdLib::script_hash(),
                "0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0",
            ),
            (
                CryptoLib::NAME,
                CryptoLib::script_hash(),
                "0x726cb6e0cd8628a1350a611384688911ab75f51b",
            ),
            (
                LedgerContract::NAME,
                LedgerContract::script_hash(),
                "0xda65b600f7124ce6c79950c1772a36403104f2be",
            ),
            (
                NeoToken::NAME,
                NeoToken::script_hash(),
                "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5",
            ),
            (
                GasToken::NAME,
                GasToken::script_hash(),
                "0xd2a4cff31913016155e38e474a2c06d08be276cf",
            ),
            (
                PolicyContract::NAME,
                PolicyContract::script_hash(),
                "0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b",
            ),
            (
                RoleManagement::NAME,
                RoleManagement::script_hash(),
                "0x49cf4e5378ffcd4dec034fd98a174c5491e395e2",
            ),
            (
                OracleContract::NAME,
                OracleContract::script_hash(),
                "0xfe924b7cfe89ddd271abaf7210a80a7e11178758",
            ),
            (
                Notary::NAME,
                Notary::script_hash(),
                "0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b",
            ),
            (
                Treasury::NAME,
                Treasury::script_hash(),
                "0x156326f25b1b5d839a4d326aeaa75383c9563ac1",
            ),
        ];

        for (name, actual, expected_hex) in expected {
            assert_eq!(actual, UInt160::parse(expected_hex).unwrap(), "{name}");
        }
    }
}

#[test]
fn standard_native_contract_catalog_matches_provider_order_and_metadata() {
    let specs = standard_native_contract_specs();
    let provider = StandardNativeProvider::new();
    let contracts = provider.all_native_contracts();

    assert_eq!(contracts.len(), specs.len());
    assert_eq!(
        specs.iter().map(|spec| spec.id).collect::<Vec<_>>(),
        (-11..=-1).rev().collect::<Vec<_>>()
    );

    for (contract, spec) in contracts.iter().zip(specs.iter()) {
        assert_eq!(contract.id(), spec.id);
        assert_eq!(contract.name(), spec.name);
        assert_eq!(contract.hash(), spec.hash);
        assert_eq!(contract.active_in(), spec.active_in);
        assert_eq!(contract.activations(), spec.activations);
        assert_eq!(
            spec.csharp_activations.first().copied().flatten(),
            spec.active_in
        );
        assert_eq!(
            provider
                .get_native_contract(&spec.hash)
                .expect("hash resolves")
                .name(),
            spec.name
        );
        assert_eq!(
            provider
                .get_native_contract_by_name(spec.name)
                .expect("name resolves")
                .hash(),
            spec.hash
        );
    }
}

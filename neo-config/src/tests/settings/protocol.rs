use super::*;

#[test]
fn csharp_default_matches_neo_v3101_protocol_settings_default() {
    let settings = ProtocolSettings::csharp_default();

    assert_eq!(settings.network, 0);
    assert_eq!(settings.address_version, constants::ADDRESS_VERSION);
    assert!(settings.standby_committee.is_empty());
    assert_eq!(settings.validators_count, 0);
    assert!(settings.seed_list.is_empty());
    assert_eq!(settings.milliseconds_per_block, 15_000);
    assert_eq!(
        settings.max_transactions_per_block,
        constants::MAX_TRANSACTIONS_PER_BLOCK as u32
    );
    assert_eq!(
        settings.max_valid_until_block_increment,
        constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT
    );
    assert_eq!(settings.memory_pool_max_transactions, 50_000);
    assert_eq!(
        settings.max_traceable_blocks,
        constants::MAX_TRACEABLE_BLOCKS
    );
    assert_eq!(
        settings.initial_gas_distribution,
        constants::INITIAL_GAS_DISTRIBUTION
    );

    for hardfork in HardforkManager::all() {
        assert_eq!(settings.hardforks.get(&hardfork), Some(&0));
    }
}

#[test]
fn load_missing_protocol_fields_uses_csharp_default_not_mainnet() {
    let json = br#"{ "ProtocolConfiguration": { "Network": 1234 } }"#;
    let mut stream = &json[..];
    let settings = ProtocolSettings::load_from_stream(&mut stream).unwrap();

    assert_eq!(settings.network, 1234);
    assert!(settings.standby_committee.is_empty());
    assert_eq!(
        settings.max_transactions_per_block,
        constants::MAX_TRANSACTIONS_PER_BLOCK as u32
    );
    assert!(settings.seed_list.is_empty());
    assert_eq!(settings.hardforks.len(), HardforkManager::all().len());
    assert!(settings.hardforks.values().all(|height| *height == 0));
}

#[test]
fn load_empty_hardforks_object_matches_v3101_default_hardforks() {
    let json = br#"{ "ProtocolConfiguration": { "Hardforks": {} } }"#;
    let mut stream = &json[..];
    let settings = ProtocolSettings::load_from_stream(&mut stream).unwrap();

    assert_eq!(settings.hardforks.len(), HardforkManager::all().len());
    for hardfork in HardforkManager::all() {
        assert_eq!(
            settings.hardforks.get(&hardfork),
            Some(&0),
            "{hardfork:?} should be enabled at genesis for an empty v3.10.1 Hardforks object"
        );
    }
    assert!(settings.is_hardfork_enabled(Hardfork::HfGorgon, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfHuyao, 0));
}

#[test]
fn load_mainnet_json_matches_neo_node_v3101_protocol_configuration() {
    let json = br#"{
        "ProtocolConfiguration": {
            "Network": 860833102,
            "AddressVersion": 53,
            "MillisecondsPerBlock": 15000,
            "MaxTransactionsPerBlock": 200,
            "MemoryPoolMaxTransactions": 50000,
            "MaxTraceableBlocks": 2102400,
            "Hardforks": {
                "HF_Aspidochelone": 1730000,
                "HF_Basilisk": 4120000,
                "HF_Cockatrice": 5450000,
                "HF_Domovoi": 5570000,
                "HF_Echidna": 7300000,
                "HF_Faun": 8800000
            },
            "InitialGasDistribution": 5200000000000000,
            "ValidatorsCount": 7,
            "StandbyCommittee": [
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
                "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
                "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554",
                "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d",
                "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e",
                "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70",
                "023a36c72844610b4d34d1968662424011bf783ca9d984efa19a20babf5582f3fe",
                "03708b860c1de5d87f5b151a12c2a99feebd2e8b315ee8e7cf8aa19692a9e18379",
                "03c6aa6e12638b36e88adc1ccdceac4db9929575c3e03576c617c49cce7114a050",
                "03204223f8c86b8cd5c89ef12e4f0dbb314172e9241e30c9ef2293790793537cf0",
                "02a62c915cf19c7f19a50ec217e79fac2439bbaad658493de0c7d8ffa92ab0aa62",
                "03409f31f0d66bdc2f70a9730b66fe186658f84a8018204db01c106edc36553cd0",
                "0288342b141c30dc8ffcde0204929bb46aed5756b41ef4a56778d15ada8f0c6654",
                "020f2887f41474cfeb11fd262e982051c1541418137c02a0f4961af911045de639",
                "0222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c30",
                "03d281b42002647f0113f36c7b8efb30db66078dfaaa9ab3ff76d043a98d512fde",
                "02504acbc1f4b3bdad1d86d6e1a08603771db135a73e61c9d565ae06a1938cd2ad",
                "0226933336f1b75baa42d42b71d9091508b638046d19abd67f4e119bf64a7cfb4d",
                "03cdcea66032b82f5c30450e381e5295cae85c5e6943af716cc6b646352a6067dc",
                "02cd5a5547119e24feaa7c2a0f37b8c9366216bab7054de0065c9be42084003c8a"
            ],
            "SeedList": [
                "seed1.neo.org:10333",
                "seed2.neo.org:10333",
                "seed3.neo.org:10333",
                "seed4.neo.org:10333",
                "seed5.neo.org:10333"
            ]
        }
    }"#;
    let mut stream = &json[..];
    let loaded = ProtocolSettings::load_from_stream(&mut stream).unwrap();
    let preset = ProtocolSettings::mainnet();

    assert_eq!(loaded, preset);
}

#[test]
fn load_testnet_json_matches_neo_node_v3101_protocol_configuration() {
    let json = br#"{
        "ProtocolConfiguration": {
            "Network": 894710606,
            "AddressVersion": 53,
            "MillisecondsPerBlock": 15000,
            "MaxTransactionsPerBlock": 5000,
            "MemoryPoolMaxTransactions": 50000,
            "MaxTraceableBlocks": 2102400,
            "Hardforks": {
                "HF_Aspidochelone": 210000,
                "HF_Basilisk": 2680000,
                "HF_Cockatrice": 3967000,
                "HF_Domovoi": 4144000,
                "HF_Echidna": 5870000,
                "HF_Faun": 12960000
            },
            "InitialGasDistribution": 5200000000000000,
            "ValidatorsCount": 7,
            "StandbyCommittee": [
                "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d",
                "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2",
                "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd",
                "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806",
                "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b",
                "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01",
                "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba",
                "025831cee3708e87d78211bec0d1bfee9f4c85ae784762f042e7f31c0d40c329b8",
                "02cf9dc6e85d581480d91e88e8cbeaa0c153a046e89ded08b4cefd851e1d7325b5",
                "03840415b0a0fcf066bcc3dc92d8349ebd33a6ab1402ef649bae00e5d9f5840828",
                "026328aae34f149853430f526ecaa9cf9c8d78a4ea82d08bdf63dd03c4d0693be6",
                "02c69a8d084ee7319cfecf5161ff257aa2d1f53e79bf6c6f164cff5d94675c38b3",
                "0207da870cedb777fceff948641021714ec815110ca111ccc7a54c168e065bda70",
                "035056669864feea401d8c31e447fb82dd29f342a9476cfd449584ce2a6165e4d7",
                "0370c75c54445565df62cfe2e76fbec4ba00d1298867972213530cae6d418da636",
                "03957af9e77282ae3263544b7b2458903624adc3f5dee303957cb6570524a5f254",
                "03d84d22b8753cf225d263a3a782a4e16ca72ef323cfde04977c74f14873ab1e4c",
                "02147c1b1d5728e1954958daff2f88ee2fa50a06890a8a9db3fa9e972b66ae559f",
                "03c609bea5a4825908027e4ab217e7efc06e311f19ecad9d417089f14927a173d5",
                "0231edee3978d46c335e851c76059166eb8878516f459e085c0dd092f0f1d51c21",
                "03184b018d6b2bc093e535519732b3fd3f7551c8cffaf4621dd5a0b89482ca66c9"
            ],
            "SeedList": [
                "seed1t5.neo.org:20333",
                "seed2t5.neo.org:20333",
                "seed3t5.neo.org:20333",
                "seed4t5.neo.org:20333",
                "seed5t5.neo.org:20333"
            ]
        }
    }"#;
    let mut stream = &json[..];
    let loaded = ProtocolSettings::load_from_stream(&mut stream).unwrap();
    let preset = ProtocolSettings::testnet();

    assert_eq!(loaded, preset);
}

#[test]
fn embedded_protocol_committees_parse_without_fallback() {
    let mainnet = ProtocolSettings::try_mainnet().unwrap();
    let testnet = ProtocolSettings::try_testnet().unwrap();

    assert_eq!(mainnet.standby_committee.len(), 21);
    assert_eq!(testnet.standby_committee.len(), 21);
}

#[test]
fn mainnet_preset_matches_neo_n3_v3101_protocol_limits() {
    let settings = ProtocolSettings::mainnet();

    assert_eq!(settings.network, constants::MAINNET_MAGIC);
    assert_eq!(settings.max_transactions_per_block, 200);
    assert_eq!(settings.hardforks.get(&Hardfork::HfFaun), Some(&8_800_000));
    // The production preset follows the explicit MainNet schedule embedded in
    // neo-rs. Unlike C# ProtocolSettings.Default / `Hardforks: {}`, omitted
    // trailing hardforks are disabled until a loaded network config schedules
    // them.
    assert!(!settings.hardforks.contains_key(&Hardfork::HfGorgon));
    assert!(!settings.hardforks.contains_key(&Hardfork::HfHuyao));
}

#[test]
fn testnet_preset_matches_neo_n3_v3101_protocol_limits() {
    let settings = ProtocolSettings::testnet();

    assert_eq!(settings.network, constants::TESTNET_MAGIC);
    assert_eq!(settings.max_transactions_per_block, 5_000);
    assert_eq!(settings.hardforks.get(&Hardfork::HfFaun), Some(&12_960_000));
    // See the MainNet test above: explicit network presets leave trailing
    // hardforks unscheduled unless the loaded config declares them.
    assert!(!settings.hardforks.contains_key(&Hardfork::HfGorgon));
    assert!(!settings.hardforks.contains_key(&Hardfork::HfHuyao));
}

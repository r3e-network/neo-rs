use neo_crypto::ECPoint;
use neo_primitives::{Hardfork, constants};
use neo_protocol::hardfork::HardforkManager;
use neo_protocol::protocol_settings::ProtocolSettings;
use std::io::{self, Cursor, Read, Write};
use std::time::Duration;
use tempfile::NamedTempFile;

fn create_hf_settings(hf: &str) -> String {
    format!(
        r#"{{
    "ProtocolConfiguration": {{
        "Network": 860833102,
        "AddressVersion": 53,
        "MillisecondsPerBlock": 15000,
        "MaxTransactionsPerBlock": 512,
        "MemoryPoolMaxTransactions": 50000,
        "MaxTraceableBlocks": 2102400,
        "Hardforks": {{
{hf}
        }},
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
    }}
}}"#
    )
}

fn load_settings(json: &str) -> Result<ProtocolSettings, String> {
    let mut file = NamedTempFile::new().expect("temp file");
    file.write_all(json.as_bytes()).expect("write config");
    ProtocolSettings::load(
        file.path()
            .to_str()
            .expect("temp file path should be valid UTF-8"),
    )
}

fn stream_settings(json: &str) -> Result<ProtocolSettings, String> {
    ProtocolSettings::load_from_stream(&mut json.as_bytes())
}

#[test]
fn hardfork_test_b_and_not_a() {
    let json = create_hf_settings("\"HF_Basilisk\": 4120000");
    let settings = load_settings(&json).expect("settings");

    assert_eq!(settings.hardforks[&Hardfork::HfAspidochelone], 0);
    assert_eq!(settings.hardforks[&Hardfork::HfBasilisk], 4_120_000);

    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 10));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfBasilisk, 0));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfBasilisk, 10));
    assert!(settings.is_hardfork_enabled(Hardfork::HfBasilisk, 4_120_000));
}

#[test]
fn hardfork_test_a_and_not_b() {
    let json = create_hf_settings("\"HF_Aspidochelone\": 0");
    let settings = load_settings(&json).expect("settings");

    assert_eq!(settings.hardforks[&Hardfork::HfAspidochelone], 0);
    assert!(!settings.hardforks.contains_key(&Hardfork::HfBasilisk));

    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 10));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfBasilisk, 0));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfBasilisk, 10));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfBasilisk, 4_120_000));
}

#[test]
fn hardfork_test_none() {
    let json = create_hf_settings("");
    let settings = load_settings(&json).expect("settings");

    assert_eq!(settings.hardforks[&Hardfork::HfAspidochelone], 0);
    assert_eq!(settings.hardforks[&Hardfork::HfBasilisk], 0);

    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfAspidochelone, 10));
    assert!(settings.is_hardfork_enabled(Hardfork::HfBasilisk, 0));
    assert!(settings.is_hardfork_enabled(Hardfork::HfBasilisk, 10));
    assert!(settings.is_hardfork_enabled(Hardfork::HfFaun, 10));
    assert!(settings.is_hardfork_enabled(Hardfork::HfGorgon, 10));
}

#[test]
fn hardfork_test_a_more_than_b() {
    let json = create_hf_settings("\"HF_Aspidochelone\": 4120001, \"HF_Basilisk\": 4120000");
    let result = load_settings(&json);
    assert!(result.is_err(), "Expected hardfork order validation error");
}

#[test]
fn fallback_settings_and_empty_manager_have_distinct_activation_semantics() {
    let settings = ProtocolSettings::default_settings();
    assert_eq!(ProtocolSettings::default(), settings);
    assert_eq!(settings.network, 0);
    assert_eq!(settings.address_version, constants::ADDRESS_VERSION);
    assert!(settings.standby_committee.is_empty());
    assert!(settings.seed_list.is_empty());
    assert_eq!(settings.validators_count, 0);
    assert_eq!(settings.milliseconds_per_block, 15_000);
    assert_eq!(settings.max_transactions_per_block, 512);
    assert_eq!(settings.max_block_size, constants::MAX_BLOCK_SIZE as u32);
    assert_eq!(settings.max_valid_until_block_increment, 5_760);
    assert_eq!(settings.memory_pool_max_transactions, 50_000);
    assert_eq!(
        settings.max_traceable_blocks,
        constants::MAX_TRACEABLE_BLOCKS
    );
    assert_eq!(
        settings.initial_gas_distribution,
        constants::INITIAL_GAS_DISTRIBUTION
    );
    assert_eq!(settings.hardforks.len(), Hardfork::ALL.len());

    for manager in [HardforkManager::new(), HardforkManager::default()] {
        assert!(manager.get_hardforks().is_empty());
        for &fork in &Hardfork::ALL {
            assert_eq!(settings.hardforks.get(&fork), Some(&0));
            assert!(settings.is_hardfork_defined(fork));
            assert!(settings.is_hardfork_enabled(fork, 0));
            assert!(settings.is_hardfork_enabled(fork, u32::MAX));
            assert!(!manager.is_enabled(fork, 0));
            assert!(!manager.is_enabled(fork, u32::MAX));
        }
    }
}

#[test]
fn manager_registration_replaces_heights_and_preserves_inclusive_boundaries() {
    let mut manager = HardforkManager::new();
    manager.register(Hardfork::HfHuyao, u32::MAX);
    assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX - 1));
    assert!(manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
    manager.register(Hardfork::HfHuyao, 0);
    assert_eq!(manager.get_hardforks().len(), 1);
    assert_eq!(manager.get_hardforks().get(&Hardfork::HfHuyao), Some(&0));
    assert!(manager.is_enabled(Hardfork::HfHuyao, 0));
    assert!(!manager.is_enabled(Hardfork::HfGorgon, u32::MAX));
}

#[test]
fn manager_enumeration_is_the_primitive_authority() {
    assert_eq!(HardforkManager::all(), &Hardfork::ALL);
    let canonical: Hardfork = neo_protocol::hardfork::Hardfork::HfHuyao;
    assert_eq!(canonical, Hardfork::HfHuyao);
}

#[test]
fn network_settings_and_managers_use_shared_config_without_changing_key_bytes() {
    for (settings, manager, config) in [
        (
            ProtocolSettings::mainnet(),
            HardforkManager::mainnet(),
            neo_config::ProtocolSettings::mainnet(),
        ),
        (
            ProtocolSettings::testnet(),
            HardforkManager::testnet(),
            neo_config::ProtocolSettings::testnet(),
        ),
    ] {
        assert_eq!(settings.network, config.network);
        assert_eq!(settings.address_version, config.address_version);
        assert_eq!(settings.validators_count as u32, config.validators_count);
        assert_eq!(settings.milliseconds_per_block as u64, config.ms_per_block);
        assert_eq!(settings.time_per_block(), config.time_per_block());
        assert_eq!(
            settings.max_valid_until_block_increment,
            config.max_valid_until_block_increment
        );
        assert_eq!(
            settings.max_transactions_per_block,
            config.max_transactions_per_block
        );
        assert_eq!(settings.max_block_size, constants::MAX_BLOCK_SIZE as u32);
        assert_eq!(
            settings.memory_pool_max_transactions as u32,
            config.memory_pool_max_transactions
        );
        assert_eq!(settings.max_traceable_blocks, config.max_traceable_blocks);
        assert_eq!(
            settings.initial_gas_distribution as i64,
            config.initial_gas_distribution
        );
        assert_eq!(settings.seed_list, config.seed_list);
        assert_eq!(
            settings.committee_members_count(),
            config.standby_validators.len()
        );
        for (point, encoded) in settings
            .standby_committee
            .iter()
            .zip(&config.standby_validators)
        {
            assert_eq!(
                point.to_bytes(),
                hex::decode(encoded).expect("config public key")
            );
        }

        let heights = serde_json::to_value(&config.hardforks).expect("config heights");
        let mut configured = 0;
        for (name, height) in heights.as_object().expect("height object") {
            let fork: Hardfork = name.parse().expect("primitive fork identity");
            if let Some(height) = height.as_u64() {
                configured += 1;
                let height = height as u32;
                assert_eq!(settings.hardforks.get(&fork), Some(&height));
                assert_eq!(manager.get_hardforks().get(&fork), Some(&height));
                assert!(settings.is_hardfork_defined(fork));
                assert!(settings.is_hardfork_enabled(fork, height));
                assert!(manager.is_enabled(fork, height));
                if height > 0 {
                    assert!(!settings.is_hardfork_enabled(fork, height - 1));
                    assert!(!manager.is_enabled(fork, height - 1));
                }
            } else {
                assert!(!settings.is_hardfork_defined(fork));
                assert!(!settings.is_hardfork_enabled(fork, u32::MAX));
                assert!(!manager.is_enabled(fork, u32::MAX));
            }
        }
        assert_eq!(settings.hardforks.len(), configured);
        assert_eq!(manager.get_hardforks().len(), configured);
    }
}

#[test]
fn standby_validators_preserve_owned_points_order_and_legacy_truncation() {
    let mut settings = ProtocolSettings::mainnet();
    settings.validators_count = 2;
    let validators: Vec<ECPoint> = settings.standby_validators();
    assert_eq!(validators, settings.standby_committee[..2]);
    let original = validators[0].to_bytes();
    settings.standby_committee.clear();
    assert_eq!(validators[0].to_bytes(), original);

    let mut settings = ProtocolSettings::testnet();
    settings.validators_count = 0;
    assert!(settings.standby_validators().is_empty());
    settings.validators_count = i32::MAX;
    assert_eq!(settings.standby_validators(), settings.standby_committee);
    // Preserve the existing signed-to-usize behavior; migration is not validation hardening.
    settings.validators_count = -1;
    assert_eq!(settings.standby_validators(), settings.standby_committee);
}

#[test]
fn time_per_block_retains_milliseconds_and_full_u32_range() {
    let mut settings = ProtocolSettings::default();
    for milliseconds in [0, 1, 3_000, 15_000, u32::MAX] {
        settings.milliseconds_per_block = milliseconds;
        let duration: Duration = settings.time_per_block();
        assert_eq!(duration, Duration::from_millis(milliseconds as u64));
    }
}

#[test]
fn stream_empty_whitespace_null_and_empty_objects_use_fallback_not_mainnet() {
    for json in [
        "",
        " \t\r\n",
        "null",
        "{}",
        r#"{"ProtocolConfiguration":{}}"#,
    ] {
        assert_eq!(
            stream_settings(json).expect("fallback"),
            ProtocolSettings::default()
        );
    }
    // A null nested section was not the same as a null top-level document.
    assert!(stream_settings(r#"{"ProtocolConfiguration":null}"#).is_err());
}

#[test]
fn stream_nested_section_takes_precedence_and_fields_keep_legacy_widths() {
    let settings = stream_settings(
        r#"{
        "Network": 999,
        "ProtocolConfiguration": {
            "Network": 42, "AddressVersion": 255, "ValidatorsCount": -1,
            "MillisecondsPerBlock": 4294967295, "MaxTransactionsPerBlock": 4294967295,
            "MaxValidUntilBlockIncrement": 4294967295, "MemoryPoolMaxTransactions": -2,
            "MaxTraceableBlocks": 4294967295, "InitialGasDistribution": 18446744073709551615,
            "SeedList": ["example.invalid:20333"], "MaxBlockSize": 1, "Unknown": true
        }
    }"#,
    )
    .expect("nested configuration");
    assert_eq!(settings.network, 42);
    assert_eq!(settings.address_version, u8::MAX);
    assert_eq!(settings.validators_count, -1);
    assert_eq!(settings.milliseconds_per_block, u32::MAX);
    assert_eq!(settings.max_transactions_per_block, u32::MAX);
    assert_eq!(settings.max_valid_until_block_increment, u32::MAX);
    assert_eq!(settings.memory_pool_max_transactions, -2);
    assert_eq!(settings.max_traceable_blocks, u32::MAX);
    assert_eq!(settings.initial_gas_distribution, u64::MAX);
    assert_eq!(settings.seed_list, ["example.invalid:20333"]);
    // MaxBlockSize was not a loadable JSON field; unknown fields remain ignored.
    assert_eq!(settings.max_block_size, constants::MAX_BLOCK_SIZE as u32);
}

#[test]
fn stream_raw_and_nested_configuration_match_file_loading() {
    let json = create_hf_settings("\"HF_Basilisk\": 4120000");
    let file_settings = load_settings(&json).expect("file settings");
    assert_eq!(
        stream_settings(&json).expect("stream settings"),
        file_settings
    );
    let document: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    let raw = serde_json::to_string(&document["ProtocolConfiguration"]).expect("raw JSON");
    assert_eq!(stream_settings(&raw).expect("raw settings"), file_settings);
}

#[test]
fn stream_reads_current_position_and_exhausted_reuse_returns_fallback() {
    let bytes = b"skip{\"Network\":42}";
    let mut stream = Cursor::new(bytes);
    stream.set_position(4);
    assert_eq!(
        ProtocolSettings::load_from_stream(&mut stream)
            .expect("settings")
            .network,
        42
    );
    assert_eq!(stream.position(), bytes.len() as u64);
    assert_eq!(
        ProtocolSettings::load_from_stream(&mut stream).expect("exhausted"),
        ProtocolSettings::default()
    );
}

struct OneByteReader<'a>(&'a [u8]);

impl Read for OneByteReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(1);
        self.0.read(&mut buffer[..count])
    }
}

#[test]
fn stream_accepts_short_reads_without_seek() {
    let mut reader = OneByteReader(br#"{"Network":42}"#);
    assert_eq!(
        ProtocolSettings::load_from_stream(&mut reader)
            .expect("short reader")
            .network,
        42
    );
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected read failure"))
    }
}

#[test]
fn stream_read_and_json_error_messages_are_preserved() {
    assert_eq!(
        ProtocolSettings::load_from_stream(&mut FailingReader).unwrap_err(),
        "Failed to read protocol settings stream: injected read failure"
    );
    for json in ["{", "{} trailing", "\u{feff}{}"] {
        assert!(
            stream_settings(json)
                .unwrap_err()
                .starts_with("Invalid protocol settings JSON: ")
        );
    }
    for json in [
        r#"{"Network":-1}"#,
        r#"{"AddressVersion":256}"#,
        r#"{"MillisecondsPerBlock":4294967296}"#,
        r#"{"InitialGasDistribution":-1}"#,
    ] {
        assert!(
            stream_settings(json).is_err(),
            "invalid field must fail: {json}"
        );
    }
}

#[test]
fn hardfork_interior_gap_and_decreasing_heights_keep_exact_errors() {
    assert_eq!(
        stream_settings(r#"{"Hardforks":{"HF_Aspidochelone":0,"HF_Cockatrice":5}}"#).unwrap_err(),
        "Hardfork HfCockatrice is configured while HfBasilisk is missing. Configure every hardfork sequentially."
    );
    assert_eq!(
        stream_settings(r#"{"Hardforks":{"HF_Aspidochelone":2,"HF_Basilisk":1}}"#).unwrap_err(),
        "Hardfork HfBasilisk activates at block 1, which is before previously configured height 2."
    );
}

#[test]
fn hardfork_normalization_fills_only_leading_omissions_not_trailing_forks() {
    let settings =
        stream_settings(r#"{"Hardforks":{"HF_Cockatrice":100}}"#).expect("leading omissions");
    assert_eq!(settings.hardforks.len(), 3);
    for fork in [Hardfork::HfAspidochelone, Hardfork::HfBasilisk] {
        assert_eq!(settings.hardforks.get(&fork), Some(&0));
        assert!(settings.is_hardfork_enabled(fork, 0));
    }
    assert!(settings.is_hardfork_defined(Hardfork::HfCockatrice));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfCockatrice, 99));
    assert!(settings.is_hardfork_enabled(Hardfork::HfCockatrice, 100));
    for &fork in &Hardfork::ALL[3..] {
        assert!(!settings.is_hardfork_defined(fork));
        assert!(!settings.is_hardfork_enabled(fork, u32::MAX));
    }
}

#[test]
fn hardfork_equal_heights_and_u32_max_activation_are_inclusive() {
    let settings = stream_settings(
        r#"{"Hardforks":{"HF_Aspidochelone":4294967295,"HF_Basilisk":4294967295}}"#,
    )
    .expect("equal heights");
    for fork in [Hardfork::HfAspidochelone, Hardfork::HfBasilisk] {
        assert!(!settings.is_hardfork_enabled(fork, u32::MAX - 1));
        assert!(settings.is_hardfork_enabled(fork, u32::MAX));
        assert!(settings.is_hardfork_defined(fork));
    }
    assert!(!settings.is_hardfork_defined(Hardfork::HfCockatrice));
    assert!(!settings.is_hardfork_enabled(Hardfork::HfCockatrice, u32::MAX));
}

#[test]
fn hardfork_names_use_primitive_aliases_and_unknown_name_error() {
    let settings =
        stream_settings(r#"{"Hardforks":{" asp ":0,"bAsIlIsK":10}}"#).expect("primitive aliases");
    assert_eq!(settings.hardforks.get(&Hardfork::HfAspidochelone), Some(&0));
    assert_eq!(settings.hardforks.get(&Hardfork::HfBasilisk), Some(&10));
    assert_eq!(
        stream_settings(r#"{"Hardforks":{"Future":1}}"#).unwrap_err(),
        "Future: Unknown hardfork: 'Future'"
    );
}

#[test]
fn committee_hex_normalization_preserves_order_and_encoded_bytes() {
    let keys = neo_config::ProtocolSettings::mainnet().standby_validators;
    let raw = serde_json::json!({"StandbyCommittee": [
        format!(" 0x{} ", keys[0]), "", "  ", format!("0X{}", keys[1])
    ]});
    let settings = stream_settings(&raw.to_string()).expect("normalized keys");
    assert_eq!(settings.committee_members_count(), 2);
    for (point, key) in settings.standby_committee.iter().zip(&keys[..2]) {
        assert_eq!(point.to_bytes(), hex::decode(key).expect("key bytes"));
    }
}

#[test]
fn committee_hex_and_point_errors_remain_string_errors() {
    assert!(
        stream_settings(r#"{"StandbyCommittee":["not hex"]}"#)
            .unwrap_err()
            .starts_with("Invalid ECPoint hex 'not hex': ")
    );
    let expected = ECPoint::from_bytes(&[1]).unwrap_err().to_string();
    assert_eq!(
        stream_settings(r#"{"StandbyCommittee":["01"]}"#).unwrap_err(),
        expected
    );
}

#[test]
fn missing_file_returns_fallback_and_absolute_discovery_prefers_requested_root() {
    let directory = tempfile::tempdir().expect("temp directory");
    let missing = directory.path().join("missing-protocol-settings.json");
    assert_eq!(
        ProtocolSettings::load(missing.to_str().expect("path")).expect("missing file"),
        ProtocolSettings::default()
    );
    assert_eq!(
        ProtocolSettings::find_file(
            "missing-protocol-settings.json",
            directory.path().to_str().expect("directory")
        ),
        None
    );

    let mut file = NamedTempFile::new_in(directory.path()).expect("temp file");
    file.write_all(br#"{"Network":42}"#).expect("write");
    let name = file
        .path()
        .file_name()
        .expect("name")
        .to_str()
        .expect("UTF-8 name");
    let resolved: Option<String> =
        ProtocolSettings::find_file(name, directory.path().to_str().expect("directory"));
    assert_eq!(resolved.as_deref(), file.path().to_str());
    assert_eq!(
        ProtocolSettings::load(resolved.as_deref().expect("resolved file"))
            .expect("file")
            .network,
        42
    );
}

#[test]
fn file_discovery_uses_executable_relative_roots_and_fallback_without_changing_cwd() {
    let executable = std::env::current_exe().expect("executable");
    let name = executable
        .file_name()
        .expect("name")
        .to_str()
        .expect("UTF-8 name");
    let directory = tempfile::tempdir().expect("temp directory");
    let cwd = std::env::current_dir().expect("working directory");
    let relative = executable
        .parent()
        .expect("executable root")
        .join(".")
        .join(name);
    assert_eq!(
        ProtocolSettings::find_file(name, "."),
        Some(relative.to_string_lossy().to_string())
    );
    assert_eq!(
        ProtocolSettings::find_file(name, directory.path().to_str().expect("directory")),
        Some(executable.to_string_lossy().to_string())
    );
    assert_eq!(std::env::current_dir().expect("working directory"), cwd);
}

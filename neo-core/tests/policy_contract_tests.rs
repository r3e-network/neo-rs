//! PolicyContract unit tests matching C# UT_PolicyContract
//!
//! Tests for Neo.SmartContract.Native.PolicyContract functionality.

use neo_core::hardfork::Hardfork;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::native::policy_contract::PolicyContract;
use neo_core::smart_contract::native::NativeContract;
use neo_core::smart_contract::ContractParameterType;
use std::collections::HashMap;

/// Tests that PolicyContract has correct contract ID (-7)
#[test]
fn test_policy_contract_id() {
    let policy = PolicyContract::new();
    assert_eq!(policy.id(), -7, "PolicyContract ID should be -7");
}

/// Tests that PolicyContract has correct name
#[test]
fn test_policy_contract_name() {
    let policy = PolicyContract::new();
    assert_eq!(
        policy.name(),
        "PolicyContract",
        "PolicyContract name should match"
    );
}

/// Tests default fee per byte value (1000 datoshi)
#[test]
fn test_default_fee_per_byte() {
    assert_eq!(
        PolicyContract::DEFAULT_FEE_PER_BYTE,
        1000,
        "Default fee per byte should be 1000 datoshi"
    );
}

/// Tests default execution fee factor (30)
#[test]
fn test_default_exec_fee_factor() {
    assert_eq!(
        PolicyContract::DEFAULT_EXEC_FEE_FACTOR,
        30,
        "Default exec fee factor should be 30"
    );
}

/// Tests default storage price (100000)
#[test]
fn test_default_storage_price() {
    assert_eq!(
        PolicyContract::DEFAULT_STORAGE_PRICE,
        100000,
        "Default storage price should be 100000"
    );
}

/// Tests default NotaryAssisted attribute fee (after Echidna)
#[test]
fn test_default_notary_assisted_attribute_fee() {
    assert_eq!(
        PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE,
        10_000_000,
        "Default NotaryAssisted attribute fee should be 10,000,000 datoshi"
    );
}

/// Tests maximum traceable blocks (about 1 year)
#[test]
fn test_max_traceable_blocks() {
    assert_eq!(
        PolicyContract::MAX_MAX_TRACEABLE_BLOCKS,
        2_102_400,
        "Max traceable blocks should be 2,102,400 (about 1 year)"
    );
}

/// Tests maximum valid until block increment
#[test]
fn test_max_valid_until_block_increment() {
    assert_eq!(
        PolicyContract::MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
        86_400,
        "Max valid until block increment should be 86,400"
    );
}

/// Tests that PolicyContract methods are registered
#[test]
fn test_policy_contract_methods() {
    let policy = PolicyContract::new();
    let methods = policy.methods();

    // Verify key methods are registered
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(
        method_names.contains(&"getFeePerByte"),
        "Should have getFeePerByte"
    );
    assert!(
        method_names.contains(&"getExecFeeFactor"),
        "Should have getExecFeeFactor"
    );
    assert!(
        method_names.contains(&"getStoragePrice"),
        "Should have getStoragePrice"
    );
    assert!(
        method_names.contains(&"getAttributeFee"),
        "Should have getAttributeFee"
    );
    assert!(
        method_names.contains(&"setFeePerByte"),
        "Should have setFeePerByte"
    );
    assert!(
        method_names.contains(&"setExecFeeFactor"),
        "Should have setExecFeeFactor"
    );
    assert!(
        method_names.contains(&"setStoragePrice"),
        "Should have setStoragePrice"
    );
    assert!(
        method_names.contains(&"setAttributeFee"),
        "Should have setAttributeFee"
    );
    assert!(
        method_names.contains(&"blockAccount"),
        "Should have blockAccount"
    );
    assert!(
        method_names.contains(&"unblockAccount"),
        "Should have unblockAccount"
    );
    assert!(method_names.contains(&"isBlocked"), "Should have isBlocked");

    // Hardforked APIs
    assert!(
        method_names.contains(&"getMillisecondsPerBlock"),
        "Should have getMillisecondsPerBlock"
    );
    assert!(
        method_names.contains(&"setMillisecondsPerBlock"),
        "Should have setMillisecondsPerBlock"
    );
    assert!(
        method_names.contains(&"getMaxValidUntilBlockIncrement"),
        "Should have getMaxValidUntilBlockIncrement"
    );
    assert!(
        method_names.contains(&"setMaxValidUntilBlockIncrement"),
        "Should have setMaxValidUntilBlockIncrement"
    );
    assert!(
        method_names.contains(&"getMaxTraceableBlocks"),
        "Should have getMaxTraceableBlocks"
    );
    assert!(
        method_names.contains(&"setMaxTraceableBlocks"),
        "Should have setMaxTraceableBlocks"
    );
    assert!(
        method_names.contains(&"getBlockedAccounts"),
        "Should have getBlockedAccounts"
    );

    // Removed N3 APIs (legacy divergence in old Rust implementation)
    assert!(
        !method_names.contains(&"getMaxTransactionsPerBlock"),
        "Should not have getMaxTransactionsPerBlock"
    );
    assert!(
        !method_names.contains(&"getMaxBlockSize"),
        "Should not have getMaxBlockSize"
    );
    assert!(
        !method_names.contains(&"getMaxBlockSystemFee"),
        "Should not have getMaxBlockSystemFee"
    );
}

#[test]
fn test_policy_contract_method_metadata_matches_protocol() {
    let policy = PolicyContract::new();
    let expected_methods: &[(
        &str,
        bool,
        u8,
        &[ContractParameterType],
        ContractParameterType,
        Option<Hardfork>,
        Option<Hardfork>,
        &[&str],
    )] = &[
        (
            "getFeePerByte",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            None,
            None,
            &[],
        ),
        (
            "getExecFeeFactor",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            None,
            None,
            &[],
        ),
        (
            "getExecPicoFeeFactor",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            Some(Hardfork::HfFaun),
            None,
            &[],
        ),
        (
            "getStoragePrice",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            None,
            None,
            &[],
        ),
        (
            "getMillisecondsPerBlock",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            Some(Hardfork::HfEchidna),
            None,
            &[],
        ),
        (
            "getMaxValidUntilBlockIncrement",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            Some(Hardfork::HfEchidna),
            None,
            &[],
        ),
        (
            "getMaxTraceableBlocks",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::Integer,
            Some(Hardfork::HfEchidna),
            None,
            &[],
        ),
        (
            "getAttributeFee",
            true,
            CallFlags::READ_STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Integer,
            None,
            Some(Hardfork::HfEchidna),
            &["attributeType"],
        ),
        (
            "getAttributeFee",
            true,
            CallFlags::READ_STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Integer,
            Some(Hardfork::HfEchidna),
            None,
            &["attributeType"],
        ),
        (
            "setFeePerByte",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            None,
            None,
            &["value"],
        ),
        (
            "setExecFeeFactor",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            None,
            None,
            &["value"],
        ),
        (
            "setStoragePrice",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            None,
            None,
            &["value"],
        ),
        (
            "setMillisecondsPerBlock",
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            Some(Hardfork::HfEchidna),
            None,
            &["value"],
        ),
        (
            "setMaxValidUntilBlockIncrement",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            Some(Hardfork::HfEchidna),
            None,
            &["value"],
        ),
        (
            "setMaxTraceableBlocks",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Integer],
            ContractParameterType::Void,
            Some(Hardfork::HfEchidna),
            None,
            &["value"],
        ),
        (
            "setAttributeFee",
            false,
            CallFlags::STATES.bits(),
            &[
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
            None,
            Some(Hardfork::HfEchidna),
            &["attributeType", "value"],
        ),
        (
            "setAttributeFee",
            false,
            CallFlags::STATES.bits(),
            &[
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
            Some(Hardfork::HfEchidna),
            None,
            &["attributeType", "value"],
        ),
        (
            "isBlocked",
            true,
            CallFlags::READ_STATES.bits(),
            &[ContractParameterType::Hash160],
            ContractParameterType::Boolean,
            None,
            None,
            &["account"],
        ),
        (
            "blockAccount",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Hash160],
            ContractParameterType::Boolean,
            None,
            Some(Hardfork::HfFaun),
            &["account"],
        ),
        (
            "blockAccount",
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            &[ContractParameterType::Hash160],
            ContractParameterType::Boolean,
            Some(Hardfork::HfFaun),
            None,
            &["account"],
        ),
        (
            "unblockAccount",
            false,
            CallFlags::STATES.bits(),
            &[ContractParameterType::Hash160],
            ContractParameterType::Boolean,
            None,
            None,
            &["account"],
        ),
        (
            "getBlockedAccounts",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::InteropInterface,
            Some(Hardfork::HfFaun),
            None,
            &[],
        ),
        (
            "setWhitelistFeeContract",
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            &[
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
            Some(Hardfork::HfFaun),
            None,
            &["contractHash", "method", "argCount", "fixedFee"],
        ),
        (
            "removeWhitelistFeeContract",
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            &[
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
            Some(Hardfork::HfFaun),
            None,
            &["contractHash", "method", "argCount"],
        ),
        (
            "getWhitelistFeeContracts",
            true,
            CallFlags::READ_STATES.bits(),
            &[],
            ContractParameterType::InteropInterface,
            Some(Hardfork::HfFaun),
            None,
            &[],
        ),
        (
            "recoverFund",
            false,
            CallFlags::ALL.bits(),
            &[
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
            Some(Hardfork::HfFaun),
            None,
            &["account", "token"],
        ),
    ];

    assert_eq!(policy.methods().len(), expected_methods.len());
    for (method, (name, safe, flags, parameters, return_type, active_in, deprecated_in, names)) in
        policy.methods().iter().zip(expected_methods.iter())
    {
        assert_eq!(method.name.as_str(), *name);
        assert_eq!(method.cpu_fee, 1 << 15, "{name}");
        assert_eq!(method.storage_fee, 0, "{name}");
        assert_eq!(method.safe, *safe, "{name}");
        assert_eq!(method.required_call_flags, *flags, "{name}");
        assert_eq!(method.parameters.as_slice(), *parameters, "{name}");
        assert_eq!(&method.return_type, return_type, "{name}");
        assert_eq!(method.active_in, *active_in, "{name}");
        assert_eq!(method.deprecated_in, *deprecated_in, "{name}");
        let actual_names = method
            .parameter_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(actual_names, *names, "{name}");
    }
}

#[test]
fn test_policy_contract_event_metadata_hardforks() {
    let policy = PolicyContract::new();
    let settings = policy_event_settings();

    assert!(policy.events(&settings, 19).is_empty());

    let echidna_events = policy.events(&settings, 20);
    assert_event_descriptors(
        &echidna_events,
        &[(
            "MillisecondsPerBlockChanged",
            &[
                ("old", ContractParameterType::Integer),
                ("new", ContractParameterType::Integer),
            ],
        )],
    );

    let faun_events = policy.events(&settings, 40);
    assert_event_descriptors(
        &faun_events,
        &[
            (
                "MillisecondsPerBlockChanged",
                &[
                    ("old", ContractParameterType::Integer),
                    ("new", ContractParameterType::Integer),
                ],
            ),
            (
                "WhitelistFeeChanged",
                &[
                    ("contract", ContractParameterType::Hash160),
                    ("method", ContractParameterType::String),
                    ("argCount", ContractParameterType::Integer),
                    ("fee", ContractParameterType::Any),
                ],
            ),
            (
                "RecoveredFund",
                &[("account", ContractParameterType::Hash160)],
            ),
        ],
    );
}

fn policy_event_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks = HashMap::from([
        (Hardfork::HfAspidochelone, 0),
        (Hardfork::HfBasilisk, 0),
        (Hardfork::HfCockatrice, 0),
        (Hardfork::HfDomovoi, 0),
        (Hardfork::HfEchidna, 20),
        (Hardfork::HfFaun, 40),
        (Hardfork::HfGorgon, 60),
    ]);
    settings
}

fn assert_event_descriptors(
    events: &[neo_core::smart_contract::manifest::ContractEventDescriptor],
    expected: &[(&str, &[(&str, ContractParameterType)])],
) {
    assert_eq!(events.len(), expected.len());
    for (event, (name, parameters)) in events.iter().zip(expected.iter()) {
        assert_eq!(event.name.as_str(), *name);
        assert_eq!(event.parameters.len(), parameters.len(), "{name}");
        for (parameter, (expected_name, expected_type)) in
            event.parameters.iter().zip(parameters.iter())
        {
            assert_eq!(parameter.name.as_str(), *expected_name, "{name}");
            assert_eq!(&parameter.param_type, expected_type, "{name}");
        }
    }
}

/// Tests that safe methods have correct flags
#[test]
fn test_safe_method_flags() {
    let policy = PolicyContract::new();
    let methods = policy.methods();

    // Find getFeePerByte - should be safe
    let get_fee = methods.iter().find(|m| m.name == "getFeePerByte");
    assert!(get_fee.is_some(), "getFeePerByte should exist");
    assert!(get_fee.unwrap().safe, "getFeePerByte should be safe");
}

/// Tests that unsafe methods have correct flags
#[test]
fn test_unsafe_method_flags() {
    let policy = PolicyContract::new();
    let methods = policy.methods();

    // Find setFeePerByte - should be unsafe
    let set_fee = methods.iter().find(|m| m.name == "setFeePerByte");
    assert!(set_fee.is_some(), "setFeePerByte should exist");
    assert!(!set_fee.unwrap().safe, "setFeePerByte should be unsafe");
}

/// Tests default attribute fee is 0
#[test]
fn test_default_attribute_fee() {
    assert_eq!(
        PolicyContract::DEFAULT_ATTRIBUTE_FEE,
        0,
        "Default attribute fee should be 0"
    );
}

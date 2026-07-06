//! # neo-native-contracts::tests::style
//!
//! Test module grouping style behavior coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `events`: Mempool event records emitted to subscribers.

use crate::{
    NEP17_STANDARD, NEP17_TRANSFER_EVENT, NEP26_STANDARD, NEP27_STANDARD, NEP30_STANDARD,
    STANDARD_NATIVE_CONTRACT_COUNT,
};

#[path = "events.rs"]
mod events;

pub(super) fn standard_contract_sources()
-> [(&'static str, &'static str); STANDARD_NATIVE_CONTRACT_COUNT] {
    [
        (
            "ContractManagement",
            concat!(
                include_str!("../../contract_management/operations/storage.rs"),
                "\n",
                include_str!("../../contract_management/operations/validation.rs"),
                "\n",
                include_str!("../../contract_management/operations.rs"),
                "\n",
                include_str!("../../contract_management/metadata.rs"),
                "\n",
                include_str!("../../contract_management/mod.rs"),
            ),
        ),
        (
            "StdLib",
            concat!(
                include_str!("../../std_lib/encoding.rs"),
                "\n",
                include_str!("../../std_lib/serialization.rs"),
                "\n",
                include_str!("../../std_lib/metadata.rs"),
                "\n",
                include_str!("../../std_lib/mod.rs"),
            ),
        ),
        (
            "CryptoLib",
            concat!(
                include_str!("../../crypto_lib/metadata.rs"),
                "\n",
                include_str!("../../crypto_lib/mod.rs")
            ),
        ),
        (
            "LedgerContract",
            concat!(
                include_str!("../../ledger_contract/storage.rs"),
                "\n",
                include_str!("../../ledger_contract/wire.rs"),
                "\n",
                include_str!("../../ledger_contract/metadata.rs"),
                "\n",
                include_str!("../../ledger_contract/mod.rs"),
            ),
        ),
        (
            "NeoToken",
            concat!(
                include_str!("../../neo_token/storage/mod.rs"),
                "\n",
                include_str!("../../neo_token/storage/views.rs"),
                "\n",
                include_str!("../../neo_token/transfers.rs"),
                "\n",
                include_str!("../../neo_token/fast_forward.rs"),
                "\n",
                include_str!("../../neo_token/metadata.rs"),
                "\n",
                include_str!("../../neo_token/invoke.rs"),
                "\n",
                include_str!("../../neo_token/mod.rs"),
            ),
        ),
        (
            "GasToken",
            concat!(
                include_str!("../../gas_token/metadata.rs"),
                "\n",
                include_str!("../../gas_token/invoke.rs"),
                "\n",
                include_str!("../../gas_token/storage.rs"),
                "\n",
                include_str!("../../gas_token/mod.rs"),
            ),
        ),
        (
            "PolicyContract",
            concat!(
                include_str!("../../policy_contract/storage/recovery.rs"),
                "\n",
                include_str!("../../policy_contract/storage/whitelist.rs"),
                "\n",
                include_str!("../../policy_contract/storage.rs"),
                "\n",
                include_str!("../../policy_contract/dispatch.rs"),
                "\n",
                include_str!("../../policy_contract/metadata.rs"),
                "\n",
                include_str!("../../policy_contract/mod.rs"),
            ),
        ),
        (
            "RoleManagement",
            concat!(
                include_str!("../../role_management/storage.rs"),
                "\n",
                include_str!("../../role_management/node_list.rs"),
                "\n",
                include_str!("../../role_management/metadata.rs"),
                "\n",
                include_str!("../../role_management/mod.rs"),
            ),
        ),
        (
            "OracleContract",
            concat!(
                include_str!("../../oracle_contract/request.rs"),
                "\n",
                include_str!("../../oracle_contract/storage.rs"),
                "\n",
                include_str!("../../oracle_contract/invoke.rs"),
                "\n",
                include_str!("../../oracle_contract/metadata.rs"),
                "\n",
                include_str!("../../oracle_contract/mod.rs"),
            ),
        ),
        (
            "Notary",
            concat!(
                include_str!("../../notary/storage.rs"),
                "\n",
                include_str!("../../notary/invoke.rs"),
                "\n",
                include_str!("../../notary/metadata.rs"),
                "\n",
                include_str!("../../notary/mod.rs"),
            ),
        ),
        (
            "Treasury",
            concat!(
                include_str!("../../treasury/metadata.rs"),
                "\n",
                include_str!("../../treasury/mod.rs")
            ),
        ),
    ]
}

#[test]
fn native_contract_style_sources_follow_canonical_catalog_order() {
    let style_names = standard_contract_sources().map(|(name, _)| name);
    let catalog_names = crate::standard_native_contract_specs().map(|spec| spec.name);

    assert_eq!(style_names, catalog_names);
}

#[test]
fn native_contract_handles_use_uniform_macros() {
    for (name, source) in standard_contract_sources() {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("native_contract_handle!("),
            "{name} should declare its handle via native_contract_handle!"
        );
        assert!(
            production.contains(&format!("native_contract_identity!({name});")),
            "{name} should implement NativeContract identity via native_contract_identity!"
        );
        assert!(
            !production.contains("fn id(&self)"),
            "{name} should not hand-write NativeContract::id"
        );
        assert!(
            !production.contains("fn hash(&self)"),
            "{name} should not hand-write NativeContract::hash"
        );
        assert!(
            !production.contains("fn name(&self)"),
            "{name} should not hand-write NativeContract::name"
        );
        assert!(
            !production.contains("fn as_any(&self)"),
            "{name} should not hand-write NativeContract::as_any"
        );
    }
}

#[test]
fn native_contract_metadata_tables_use_handle_name_prefixes() {
    let expected = [
        ("ContractManagement", "CONTRACT_MANAGEMENT"),
        ("StdLib", "STD_LIB"),
        ("CryptoLib", "CRYPTO_LIB"),
        ("LedgerContract", "LEDGER_CONTRACT"),
        ("NeoToken", "NEO_TOKEN"),
        ("GasToken", "GAS_TOKEN"),
        ("PolicyContract", "POLICY_CONTRACT"),
        ("RoleManagement", "ROLE_MANAGEMENT"),
        ("OracleContract", "ORACLE_CONTRACT"),
        ("Notary", "NOTARY"),
        ("Treasury", "TREASURY"),
    ];

    for ((name, source), (expected_name, prefix)) in
        standard_contract_sources().into_iter().zip(expected)
    {
        assert_eq!(name, expected_name);
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains(&format!("static {prefix}_METHODS")),
            "{name} metadata methods should use the full handle prefix {prefix}_METHODS"
        );
        let has_events = production.contains("fn event_descriptors(&self)");
        if has_events {
            assert!(
                production.contains(&format!("static {prefix}_EVENTS")),
                "{name} metadata events should use the full handle prefix {prefix}_EVENTS"
            );
        }
    }
}

#[test]
fn native_contracts_reference_metadata_tables_through_module_namespace() {
    for (name, source) in standard_contract_sources() {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production.contains("use metadata::"),
            "{name} should reference metadata tables as metadata::CONTRACT_TABLE for consistent native-contract style"
        );
    }
}

#[test]
fn native_contract_storage_keys_use_shared_builders() {
    for (name, source) in standard_contract_sources() {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production.contains("StorageKey::create("),
            "{name} should route single-prefix native storage keys through crate::keys"
        );
        assert!(
            !production.contains("StorageKey::create_with_bytes("),
            "{name} should route raw-suffix native storage keys through crate::keys"
        );
        assert!(
            !production.contains("StorageKey::create_with_"),
            "{name} should route typed native storage keys through crate::keys"
        );
        assert!(
            !production.contains("StorageKey::new(Self::ID"),
            "{name} should not hand-wrap raw suffixes with StorageKey::new(Self::ID, ...)"
        );
        assert!(
            !production.contains("StorageKey::new(\n            Self::ID"),
            "{name} should not hand-wrap raw suffixes with StorageKey::new(Self::ID, ...)"
        );
        assert!(
            !production.contains("StorageKey::new(\n                Self::ID"),
            "{name} should not hand-wrap raw suffixes with StorageKey::new(Self::ID, ...)"
        );
    }
}

#[test]
fn native_contract_supported_standards_use_shared_helper() {
    for (name, source) in standard_contract_sources() {
        for standard in [
            NEP17_STANDARD,
            NEP26_STANDARD,
            NEP27_STANDARD,
            NEP30_STANDARD,
        ] {
            assert!(
                !source.contains(&format!("\"{standard}\".to_string()")),
                "{name} should build manifest supported standards through native_supported_standards"
            );
        }
    }
}

#[test]
fn nep17_transfer_notifications_use_shared_event_name() {
    assert_eq!(NEP17_TRANSFER_EVENT, "Transfer");

    for (name, source) in standard_contract_sources() {
        assert!(
            !source.contains("\"Transfer\".to_string()"),
            "{name} should emit NEP-17 Transfer notifications via NEP17_TRANSFER_EVENT"
        );
    }
}

#[test]
fn nep17_transfer_notifications_use_shared_payload_helper() {
    for name in ["GasToken", "NeoToken"] {
        let source = standard_contract_sources()
            .into_iter()
            .find(|(contract_name, _)| *contract_name == name)
            .map(|(_, source)| source)
            .expect("NEP-17 token source should be available");

        assert!(
            source.contains("nep17_transfer_notification_state"),
            "{name} should build Transfer notification payloads through nep17_transfer_notification_state"
        );
        assert!(
            !source.contains("NEP17_TRANSFER_EVENT.to_owned(),\n                vec!["),
            "{name} should not hand-write Transfer notification payload vectors"
        );
    }
}

#[test]
fn nep17_token_method_tables_use_shared_abi_helpers() {
    for name in ["GasToken", "NeoToken"] {
        let source = standard_contract_sources()
            .into_iter()
            .find(|(contract_name, _)| *contract_name == name)
            .map(|(_, source)| source)
            .expect("NEP-17 token source should be available");

        for helper in [
            "nep17_symbol_method",
            "nep17_decimals_method",
            "nep17_total_supply_method",
            "nep17_balance_of_method",
            "nep17_transfer_method",
        ] {
            assert!(
                source.contains(helper),
                "{name} should build NEP-17 ABI method descriptors through {helper}"
            );
        }

        for conversion in [".into()", ".to_string()", ".to_owned()"] {
            assert!(
                !source.contains(&format!(
                    "NativeMethod::new(\n            \"transfer\"{conversion}"
                )),
                "{name} should not hand-write the NEP-17 transfer method descriptor"
            );
        }
    }
}

#[test]
fn nep17_payment_callbacks_use_shared_method_and_payload_helper() {
    for name in ["GasToken", "NeoToken"] {
        let source = standard_contract_sources()
            .into_iter()
            .find(|(contract_name, _)| *contract_name == name)
            .map(|(_, source)| source)
            .expect("NEP-17 token source should be available");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production.contains("NEP17_PAYMENT_METHOD"),
            "{name} should queue NEP-17 payment callbacks via NEP17_PAYMENT_METHOD"
        );
        assert!(
            production.contains("nep17_payment_callback_args"),
            "{name} should build NEP-17 payment callback args through nep17_payment_callback_args"
        );
        assert!(
            !production.contains("\"onNEP17Payment\",\n            vec!["),
            "{name} should not hand-write onNEP17Payment callback payload vectors"
        );
        assert!(
            production.contains("nep17_payment_data_item"),
            "{name} should decode transfer `data` through nep17_payment_data_item"
        );
        assert!(
            !production.contains(
                "BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)"
            ),
            "{name} should not hand-roll NEP-17 transfer data deserialization"
        );
    }
}

#[test]
fn nep17_amount_args_use_shared_raw_integer_parser() {
    for name in ["GasToken", "NeoToken", "Notary"] {
        let source = standard_contract_sources()
            .into_iter()
            .find(|(contract_name, _)| *contract_name == name)
            .map(|(_, source)| source)
            .expect("NEP-17 amount source should be available");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production.contains("crate::args::raw_required_integer_arg"),
            "{name} should decode NEP-17 amount args through raw_required_integer_arg"
        );
        assert!(
            !production.contains("BigInt::from_signed_bytes_le(args"),
            "{name} should not hand-roll raw NEP-17 amount integer decoding"
        );
    }
}

#[test]
fn nep_payment_method_descriptors_use_shared_helpers() {
    for name in ["NeoToken", "Notary"] {
        let source = standard_contract_sources()
            .into_iter()
            .find(|(contract_name, _)| *contract_name == name)
            .map(|(_, source)| source)
            .expect("NEP-17 payment source should be available");
        assert!(
            source.contains("nep17_payment_method"),
            "{name} should describe onNEP17Payment through nep17_payment_method"
        );
    }

    let treasury = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "Treasury")
        .map(|(_, source)| source)
        .expect("Treasury source should be available");
    assert!(
        treasury.contains("nep17_payment_method"),
        "Treasury should describe onNEP17Payment through nep17_payment_method"
    );
    assert!(
        treasury.contains("nep11_payment_method"),
        "Treasury should describe onNEP11Payment through nep11_payment_method"
    );
}

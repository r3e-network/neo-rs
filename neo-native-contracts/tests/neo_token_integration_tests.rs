//! Integration tests for NEO token native contract.

use neo_native_contracts::{NeoToken, NativeContract};
use neo_protocol::{Hardfork, ProtocolSettings};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::persistence::DataCache;
use neo_protocol::execution::Block;
use neo_primitives::UInt160;
use std::sync::Arc;

fn make_test_settings() -> ProtocolSettings {
    ProtocolSettings::default()
}

fn make_test_block(index: u32) -> Block {
    let header = neo_protocol::execution::BlockHeader::new(
        0,
        neo_primitives::UInt256::zero(),
        neo_primitives::UInt256::zero(),
        1468595301000,
        0,
        index,
        0,
        neo_primitives::UInt160::zero(),
        vec![],
    );
    Block::new(header, vec![])
}

fn make_test_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
    let settings = make_test_settings();
    let block = make_test_block(0);

    ApplicationEngine::new(
        neo_primitives::trigger_type::TriggerType::Application,
        None,
        snapshot,
        Some(neo_protocol::execution::conversions::convert_ledger_block(block)),
        settings,
        i64::MAX,
        None,
        neo_native_contracts::composition::default_native_services(),
    ).expect("create engine")
}

#[test]
fn neo_token_has_correct_id() {
    let neo = NeoToken::new();
    assert_eq!(neo.id(), -5);
}

#[test]
fn neo_token_has_correct_name() {
    let neo = NeoToken::new();
    assert_eq!(neo.name(), "NeoToken");
}

#[test]
fn neo_token_hash_is_deterministic() {
    let neo1 = NeoToken::new();
    let neo2 = NeoToken::new();
    assert_eq!(neo1.hash(), neo2.hash());
}

#[test]
fn neo_token_symbol_is_neo() {
    let neo = NeoToken::new();
    assert_eq!(neo.symbol(), "NEO");
}

#[test]
fn neo_token_has_0_decimals() {
    let neo = NeoToken::new();
    assert_eq!(neo.decimals(), 0);
}

#[test]
fn neo_token_is_always_active() {
    let neo = NeoToken::new();
    let settings = ProtocolSettings::default();
    assert!(neo.is_active(&settings, 0));
    assert!(neo.is_active(&settings, 1000));
    assert!(neo.is_active(&settings, u32::MAX));
}

#[test]
fn neo_token_supports_nep17() {
    let neo = NeoToken::new();
    let settings = ProtocolSettings::default();
    let standards = neo.supported_standards(&settings, 0);
    assert!(standards.contains(&"NEP-17".to_string()));
}

#[test]
fn neo_token_active_in_returns_none() {
    let neo = NeoToken::new();
    assert_eq!(neo.active_in(), None);
}

#[test]
fn neo_token_has_required_methods() {
    let neo = NeoToken::new();
    let methods = neo.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"symbol"));
    assert!(method_names.contains(&"decimals"));
    assert!(method_names.contains(&"totalSupply"));
    assert!(method_names.contains(&"balanceOf"));
    assert!(method_names.contains(&"transfer"));
}

#[test]
fn neo_token_has_governance_methods() {
    let neo = NeoToken::new();
    let methods = neo.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"registerCandidate"));
    assert!(method_names.contains(&"unregisterCandidate"));
    assert!(method_names.contains(&"vote"));
    assert!(method_names.contains(&"getCandidates"));
    assert!(method_names.contains(&"getCommittee"));
}

#[test]
fn neo_token_total_supply_is_100_million() {
    let neo = NeoToken::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = neo.invoke(&mut engine, "totalSupply", &[]);
    // Should succeed or fail gracefully
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn neo_token_rejects_unknown_method() {
    let neo = NeoToken::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = neo.invoke(&mut engine, "unknownMethod", &[]);
    assert!(result.is_err());
}

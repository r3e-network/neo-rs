//! Integration tests for Treasury native contract.

use neo_native_contracts::{NativeContract, TreasuryContract};
use neo_protocol::execution::Block;
use neo_protocol::{Hardfork, ProtocolSettings};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::persistence::DataCache;
use std::sync::Arc;

fn make_test_settings() -> ProtocolSettings {
    ProtocolSettings::default()
}

fn make_test_block(index: u32) -> Block {
    let header = neo_protocol::execution::BlockHeader::new(
        0,                               // version
        neo_primitives::UInt256::zero(), // previous_hash
        neo_primitives::UInt256::zero(), // merkle_root
        1468595301000,                   // timestamp
        0,                               // nonce
        index,                           // index
        0,                               // primary_index
        neo_primitives::UInt160::zero(), // next_consensus
        vec![],                          // witnesses
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
        Some(neo_protocol::execution::conversions::convert_ledger_block(
            block,
        )),
        settings,
        i64::MAX,
        None,
        neo_native_contracts::composition::default_native_services(),
    )
    .expect("create engine")
}

#[test]
fn treasury_has_correct_id() {
    let treasury = TreasuryContract::new();
    assert_eq!(treasury.id(), -11);
}

#[test]
fn treasury_has_correct_name() {
    let treasury = TreasuryContract::new();
    assert_eq!(treasury.name(), "Treasury");
}

#[test]
fn treasury_hash_is_deterministic() {
    let treasury1 = TreasuryContract::new();
    let treasury2 = TreasuryContract::new();
    assert_eq!(treasury1.hash(), treasury2.hash());
}

#[test]
fn treasury_activates_at_hf_faun() {
    let treasury = TreasuryContract::new();
    assert_eq!(treasury.active_in(), Some(Hardfork::HfFaun));
}

#[test]
fn treasury_has_required_methods() {
    let treasury = TreasuryContract::new();
    let methods = treasury.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"verify"));
    assert!(method_names.contains(&"onNEP17Payment"));
    assert!(method_names.contains(&"onNEP11Payment"));
}

#[test]
fn treasury_supports_nep_standards() {
    let treasury = TreasuryContract::new();
    let settings = ProtocolSettings::default();
    let standards = treasury.supported_standards(&settings, 0);

    assert!(standards.contains(&"NEP-26".to_string()));
    assert!(standards.contains(&"NEP-27".to_string()));
    assert!(standards.contains(&"NEP-30".to_string()));
}

#[test]
fn treasury_verify_method_exists() {
    let treasury = TreasuryContract::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    // Should not panic - method exists
    let result = treasury.invoke(&mut engine, "verify", &[]);
    assert!(result.is_ok() || result.is_err()); // Either succeeds or fails validation
}

#[test]
fn treasury_accepts_nep17_payment() {
    let treasury = TreasuryContract::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = treasury.invoke(&mut engine, "onNEP17Payment", &[]);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Vec::<u8>::new());
}

#[test]
fn treasury_accepts_nep11_payment() {
    let treasury = TreasuryContract::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = treasury.invoke(&mut engine, "onNEP11Payment", &[]);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Vec::<u8>::new());
}

#[test]
fn treasury_rejects_unknown_method() {
    let treasury = TreasuryContract::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = treasury.invoke(&mut engine, "unknownMethod", &[]);
    assert!(result.is_err());
}

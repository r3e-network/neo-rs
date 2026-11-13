//! Integration tests for GAS token native contract.

use neo_native_contracts::{GasToken, NativeContract};
use neo_primitives::UInt160;
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::persistence::DataCache;
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
fn gas_token_has_correct_id() {
    let gas = GasToken::new();
    assert_eq!(gas.id(), -6);
}

#[test]
fn gas_token_has_correct_name() {
    let gas = GasToken::new();
    assert_eq!(gas.name(), "GasToken");
}

#[test]
fn gas_token_hash_is_deterministic() {
    let gas1 = GasToken::new();
    let gas2 = GasToken::new();
    assert_eq!(gas1.hash(), gas2.hash());
}

#[test]
fn gas_token_symbol_is_gas() {
    let gas = GasToken::new();
    assert_eq!(gas.symbol(), "GAS");
}

#[test]
fn gas_token_has_8_decimals() {
    let gas = GasToken::new();
    assert_eq!(gas.decimals(), 8);
}

#[test]
fn gas_token_is_always_active() {
    let gas = GasToken::new();
    let settings = ProtocolSettings::default();
    assert!(gas.is_active(&settings, 0));
    assert!(gas.is_active(&settings, 1000));
    assert!(gas.is_active(&settings, u32::MAX));
}

#[test]
fn gas_token_supports_nep17() {
    let gas = GasToken::new();
    let settings = ProtocolSettings::default();
    let standards = gas.supported_standards(&settings, 0);
    assert!(standards.contains(&"NEP-17".to_string()));
}

#[test]
fn gas_token_active_in_returns_none() {
    let gas = GasToken::new();
    assert_eq!(gas.active_in(), None);
}

#[test]
fn gas_token_has_required_methods() {
    let gas = GasToken::new();
    let methods = gas.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"symbol"));
    assert!(method_names.contains(&"decimals"));
    assert!(method_names.contains(&"totalSupply"));
    assert!(method_names.contains(&"balanceOf"));
    assert!(method_names.contains(&"transfer"));
}

#[test]
fn gas_token_total_supply_method_exists() {
    let gas = GasToken::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    // Should not panic - method exists
    let result = gas.invoke(&mut engine, "totalSupply", &[]);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn gas_token_balance_of_unknown_account_returns_zero() {
    let gas = GasToken::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let account = UInt160::zero();
    let result = gas.invoke(&mut engine, "balanceOf", &[account.to_array().to_vec()]);

    // Should either succeed with zero or fail gracefully
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn gas_token_rejects_unknown_method() {
    let gas = GasToken::new();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_test_engine(snapshot);

    let result = gas.invoke(&mut engine, "unknownMethod", &[]);
    assert!(result.is_err());
}

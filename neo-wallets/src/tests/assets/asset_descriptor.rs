use super::*;
use neo_execution::contract_state::ContractState;
use neo_execution::native_contract::{NativeContract, build_native_contract_state};
use neo_native_contracts::{GasToken, NeoToken};
use neo_storage::{DataCache, StorageItem, StorageKey};

/// `ContractManagement.PREFIX_CONTRACT` — the per-contract storage prefix
/// (verified against `neo-native-contracts/src/contract_management.rs`).
const PREFIX_CONTRACT: u8 = 8;

/// Inserts a deployed `ContractState` for `state.hash` into `cache` under the
/// ContractManagement record key (the C# interoperable stack-item record),
/// mirroring a post-genesis snapshot so `get_contract_from_snapshot` can
/// resolve it.
fn deploy_contract_record(cache: &DataCache, state: &ContractState) {
    let record = state
        .serialize_contract_record()
        .expect("serialize contract record");

    let mut key = Vec::with_capacity(1 + 20);
    key.push(PREFIX_CONTRACT);
    key.extend_from_slice(&state.hash.to_bytes());

    cache.add(
        StorageKey::new(ContractManagement::ID, key),
        StorageItem::from_bytes(record),
    );
}

#[test]
fn nonexistent_asset_id_is_rejected() {
    // C# `TestConstructorWithNonexistAssetId`: an undeployed asset id throws
    // ArgumentException; here it maps to `invalid_argument`.
    let snapshot = Arc::new(DataCache::new(false));
    let settings = ProtocolSettings::default();
    let bogus = UInt160::from_bytes(&[0xAB; 20]).unwrap();

    let err = AssetDescriptor::new(snapshot, settings, bogus)
        .expect_err("undeployed asset must be rejected");
    assert!(
        err.to_string().contains("No asset contract found"),
        "unexpected error: {err}"
    );
}

#[test]
fn descriptor_reads_gas_metadata() {
    // C# `Check_GAS`: against a snapshot where GAS is deployed, the descriptor
    // exposes name=GasToken, symbol=GAS, decimals=8.
    // The in-VM `System.Contract.Call` resolves the target via the global
    // native-contract provider seam, so install the standard provider (the
    // same thing node startup does) before probing.
    neo_native_contracts::install();
    let cache = DataCache::new(false);
    let settings = ProtocolSettings::default();
    let gas = GasToken;
    let gas_state = build_native_contract_state(&gas, &settings, 0);
    deploy_contract_record(&cache, &gas_state);

    let snapshot = Arc::new(cache);
    let gas_hash = NativeContract::hash(&gas);

    let descriptor =
        AssetDescriptor::new(snapshot, settings, gas_hash).expect("GAS descriptor must build");

    assert_eq!(descriptor.asset_id, gas_hash);
    assert_eq!(descriptor.asset_name, "GasToken");
    assert_eq!(descriptor.to_string(), "GasToken");
    assert_eq!(descriptor.symbol, "GAS");
    assert_eq!(descriptor.decimals, 8);
}

#[test]
fn descriptor_reads_neo_metadata() {
    // C# `Check_NEO`: name=NeoToken, symbol=NEO, decimals=0 (exercises the
    // zero-decimals extraction path).
    neo_native_contracts::install();
    let cache = DataCache::new(false);
    let settings = ProtocolSettings::default();
    let neo = NeoToken;
    let neo_state = build_native_contract_state(&neo, &settings, 0);
    deploy_contract_record(&cache, &neo_state);

    let snapshot = Arc::new(cache);
    let neo_hash = NativeContract::hash(&neo);

    let descriptor =
        AssetDescriptor::new(snapshot, settings, neo_hash).expect("NEO descriptor must build");

    assert_eq!(descriptor.asset_id, neo_hash);
    assert_eq!(descriptor.asset_name, "NeoToken");
    assert_eq!(descriptor.to_string(), "NeoToken");
    assert_eq!(descriptor.symbol, "NEO");
    assert_eq!(descriptor.decimals, 0);
}

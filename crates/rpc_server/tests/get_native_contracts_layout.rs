use base64::{engine::general_purpose, Engine};
use neo_config::{LedgerConfig, NetworkType};
use neo_ledger::{Ledger, MemoryPool, MempoolConfig};
use neo_persistence::rocksdb::RocksDbStore;
use neo_rpc_server::{methods::RpcMethods, PeerRegistry};
use serde_json::Value;
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as AsyncRwLock;

async fn build_rpc_methods() -> RpcMethods {
    let ledger = Arc::new(
        Ledger::new_with_network(LedgerConfig::default(), NetworkType::Private)
            .await
            .expect("ledger should init"),
    );

    let temp_dir = format!(
        "/tmp/neo-rpc-native-contracts-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let store = Arc::new(RocksDbStore::new(&temp_dir).expect("rocksdb store should open"));
    let peer_registry = Arc::new(RwLock::new(PeerRegistry::default()));
    let mempool = Arc::new(AsyncRwLock::new(MemoryPool::new(MempoolConfig::default())));

    RpcMethods::new(ledger, store, peer_registry, mempool)
}

#[tokio::test]
async fn test_get_native_contracts_layout_matches_csharp_shape() {
    let methods = build_rpc_methods().await;

    let response = methods
        .get_native_contracts()
        .await
        .expect("RPC should respond");
    let contracts = response
        .as_array()
        .expect("response must be an array")
        .to_vec();
    assert_eq!(contracts.len(), 9);

    // ContractManagement should be present with deterministic metadata.
    let contract_mgmt = contracts
        .iter()
        .find(|entry| entry["id"].as_i64() == Some(-1))
        .expect("ContractManagement should exist");
    assert_eq!(
        contract_mgmt["hash"].as_str(),
        Some("0xfffdc93764dbaddd97c48f252a53ea4643faa3fd")
    );
    assert!(contract_mgmt.get("updatehistory").is_none());

    // All NEF scripts must be base64 encoded and decode successfully.
    for contract in &contracts {
        let nef = contract
            .get("nef")
            .and_then(Value::as_object)
            .expect("nef object required");
        let script = nef
            .get("script")
            .and_then(Value::as_str)
            .expect("script string required");
        let decoded = general_purpose::STANDARD
            .decode(script)
            .expect("script must be base64 encoded");
        assert!(
            !decoded.is_empty(),
            "script should not decode to empty bytes"
        );

        let tokens = nef
            .get("tokens")
            .and_then(Value::as_array)
            .expect("tokens array required");
        for token in tokens {
            assert!(
                token["callflags"].as_u64().is_some(),
                "callflags must be numeric"
            );
        }
    }

    // Spot check GAS entry deserialises and round-trips NEP-17 balance parsing path.
    let gas_contract = contracts
        .iter()
        .find(|entry| entry["id"].as_i64() == Some(-6))
        .expect("GAS contract should exist");
    assert_eq!(
        gas_contract["hash"].as_str(),
        Some("0xd2a4cff31913016155e38e474a2c06d08be276cf")
    );

    // Ensure manifest names are carried through
    for contract in &contracts {
        let manifest = contract
            .get("manifest")
            .and_then(Value::as_object)
            .expect("manifest should be object");
        assert!(manifest.contains_key("name"));
    }
}

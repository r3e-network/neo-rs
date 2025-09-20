use serde_json::json;

#[tokio::test]
async fn test_get_contract_state_returns_null_when_missing() {
    use neo_config::{LedgerConfig, NetworkType};
    use neo_core::UInt160;
    use neo_ledger::{Ledger, MemoryPool, MempoolConfig};
    use neo_persistence::rocksdb::RocksDbStore;
    use neo_rpc_server::{methods::RpcMethods, PeerRegistry};
    use std::sync::{Arc, RwLock};
    use tokio::sync::RwLock as AsyncRwLock;

    // Create a minimal ledger and store
    let ledger = Arc::new(
        Ledger::new_with_network(LedgerConfig::default(), NetworkType::Private)
            .await
            .expect("ledger should init"),
    );
    let temp_dir = format!(
        "/tmp/neo-rpc-getcontractstate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let store = Arc::new(RocksDbStore::new(&temp_dir).expect("rocksdb store should open"));
    let peer_registry = Arc::new(RwLock::new(PeerRegistry::default()));
    let mempool = Arc::new(AsyncRwLock::new(MemoryPool::new(MempoolConfig::default())));
    let methods = RpcMethods::new(ledger, store, peer_registry, mempool);

    // Prepare params: script hash only
    let script_hash = UInt160::zero().to_string();
    let params = json!([script_hash]);

    let resp = methods
        .get_contract_state(params)
        .await
        .expect("should return JSON");

    assert!(resp.is_null(), "expected null for missing contract state");
}

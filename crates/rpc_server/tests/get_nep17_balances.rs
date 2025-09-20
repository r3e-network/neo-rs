use neo_config::{LedgerConfig, NetworkType};
use neo_core::UInt160;
use neo_ledger::{Ledger, MemoryPool, MempoolConfig};
use neo_persistence::rocksdb::RocksDbStore;
use neo_rpc_server::{methods::RpcMethods, PeerRegistry};
use neo_smart_contract::native::fungible_token::PREFIX_ACCOUNT;
use num_bigint::BigInt;
use serde_json::json;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as AsyncRwLock;

async fn build_rpc_methods_with_ledger() -> (RpcMethods, Arc<Ledger>) {
    let ledger = Arc::new(
        Ledger::new_with_network(LedgerConfig::default(), NetworkType::Private)
            .await
            .expect("ledger should init"),
    );

    let temp_dir = format!(
        "/tmp/neo-rpc-nep17-balances-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let store = Arc::new(RocksDbStore::new(&temp_dir).expect("rocksdb store should open"));
    let peer_registry = Arc::new(RwLock::new(PeerRegistry::default()));
    let mempool = Arc::new(AsyncRwLock::new(MemoryPool::new(MempoolConfig::default())));

    let methods = RpcMethods::new(ledger.clone(), store, peer_registry, mempool);

    (methods, ledger)
}

fn encode_balance_state(amount: u128, height: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    let mut balance_bytes = BigInt::from(amount).to_signed_bytes_le();
    if balance_bytes.is_empty() {
        balance_bytes.push(0);
    }

    match balance_bytes.len() {
        len if len < 0xFD => payload.push(len as u8),
        len if len <= 0xFFFF => {
            payload.push(0xFD);
            payload.extend_from_slice(&(len as u16).to_le_bytes());
        }
        len if len <= 0xFFFF_FFFF => {
            payload.push(0xFE);
            payload.extend_from_slice(&(len as u32).to_le_bytes());
        }
        len => {
            payload.push(0xFF);
            payload.extend_from_slice(&(len as u64).to_le_bytes());
        }
    }

    payload.extend_from_slice(&balance_bytes);
    payload.extend_from_slice(&height.to_le_bytes());
    payload
}

#[tokio::test]
async fn test_get_nep17_balances_returns_populated_entries() {
    let (methods, ledger) = build_rpc_methods_with_ledger().await;

    let gas_hash =
        UInt160::from_str("d2a4cff31913016155e38e474a2c06d08be276cf").expect("valid GAS hash");
    let account = UInt160::from_bytes(&[0x11u8; 20]).expect("valid account");

    let mut storage_key = vec![PREFIX_ACCOUNT];
    storage_key.extend_from_slice(&account.as_bytes());
    let storage_value = encode_balance_state(1_000_000_000u128, 321);

    ledger
        .set_raw_storage_value(&gas_hash, storage_key, storage_value)
        .await
        .expect("storage write succeeds");

    let address = account.to_address();
    let response = methods
        .get_nep17_balances(json!([address.clone()]))
        .await
        .expect("RPC should succeed");

    let obj = response.as_object().expect("response is object");
    assert_eq!(
        obj.get("address").and_then(|v| v.as_str()),
        Some(address.as_str())
    );

    let balances = obj
        .get("balance")
        .and_then(|v| v.as_array())
        .expect("balances array");
    assert_eq!(balances.len(), 1);

    let entry = balances.first().unwrap();
    assert_eq!(
        entry.get("assethash").and_then(|v| v.as_str()),
        Some("0xd2a4cff31913016155e38e474a2c06d08be276cf")
    );
    assert_eq!(
        entry.get("amount").and_then(|v| v.as_str()),
        Some("1000000000")
    );
    assert_eq!(
        entry.get("lastupdatedblock").and_then(|v| v.as_u64()),
        Some(321)
    );

    let empty_response = methods
        .get_nep17_balances(json!(["NVVwFw6XyhtRCFQ8SpUTMdPyYt4Vd9A1XQ"]))
        .await
        .expect("RPC should succeed for other address");
    let balance_array = empty_response
        .get("balance")
        .and_then(|v| v.as_array())
        .expect("balances array");
    assert!(balance_array.is_empty());
}

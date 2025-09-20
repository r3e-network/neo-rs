use neo_core::UInt160;
use neo_vm::{CallFlags, ScriptBuilder, StackItem};
use serde_json::json;
use std::str::FromStr;

async fn setup_methods() -> neo_rpc_server::methods::RpcMethods {
    use neo_config::{LedgerConfig, NetworkType};
    use neo_ledger::{Ledger, MemoryPool, MempoolConfig};
    use neo_persistence::rocksdb::RocksDbStore;
    use neo_rpc_server::{methods::RpcMethods, PeerRegistry};
    use std::sync::{Arc, RwLock};
    use tokio::sync::RwLock as AsyncRwLock;

    let ledger = Arc::new(
        Ledger::new_with_network(LedgerConfig::default(), NetworkType::Private)
            .await
            .expect("ledger should init"),
    );

    let temp_dir = format!(
        "/tmp/neo-rpc-invoke-layout-{}-{}",
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

fn build_expected_script(
    hash: &UInt160,
    operation: &str,
    args: &[StackItem],
    flags: CallFlags,
) -> String {
    let mut builder = ScriptBuilder::new();
    for item in args.iter().rev() {
        builder
            .emit_push_stack_item(item.clone())
            .expect("stack item serializes");
    }
    builder.emit_push_int(args.len() as i64);
    builder.emit_pack();
    builder.emit_push_int(flags.0 as i64);
    builder.emit_push_string(operation);
    let hash_bytes = hash.as_bytes();
    builder.emit_push_byte_array(&hash_bytes);
    builder
        .emit_syscall("System.Contract.Call")
        .expect("syscall encodes");
    hex::encode(builder.to_array())
}

#[tokio::test]
async fn test_invoke_function_script_layout() {
    let methods = setup_methods().await;
    let script_hash = "0x0000000000000000000000000000000000000000";
    let operation = "balanceOf";
    let params = json!([script_hash, operation, json!([])]);

    let resp = methods
        .invoke_function(params)
        .await
        .expect("invoke_function should return a JSON response");

    let obj = resp.as_object().expect("response should be object");
    let script_hex = obj.get("script").and_then(|v| v.as_str()).unwrap();

    let hash = UInt160::from_str(script_hash).unwrap();
    let expected = build_expected_script(&hash, operation, &[], CallFlags::ALL);
    assert_eq!(script_hex, expected);
}

#[tokio::test]
async fn test_invoke_function_encodes_arguments() {
    let methods = setup_methods().await;
    let script_hash = "0x1111111111111111111111111111111111111111";
    let operation = "method";
    let params = json!([
        script_hash,
        operation,
        [
            {"type": "String", "value": "hello"},
            {"type": "Integer", "value": "42"},
            {"type": "Boolean", "value": true}
        ]
    ]);

    let resp = methods
        .invoke_function(params)
        .await
        .expect("invoke_function should succeed");
    let obj = resp.as_object().unwrap();
    let script_hex = obj.get("script").and_then(|v| v.as_str()).unwrap();

    let hash = UInt160::from_str(script_hash).unwrap();
    let args = vec![
        StackItem::from_byte_string("hello".as_bytes().to_vec()),
        StackItem::from_int(42),
        StackItem::Boolean(true),
    ];
    let expected = build_expected_script(&hash, operation, &args, CallFlags::ALL);
    assert_eq!(script_hex, expected);
}

#[tokio::test]
async fn test_invoke_function_invalid_argument_type() {
    let methods = setup_methods().await;
    let params = json!([
        "0x0000000000000000000000000000000000000000",
        "op",
        [{"value": "hello"}]
    ]);

    let err = methods.invoke_function(params).await.unwrap_err();
    assert!(err.to_string().contains("Contract parameter missing type"));
}

#[tokio::test]
async fn test_invoke_function_custom_call_flags() {
    let methods = setup_methods().await;
    let script_hash = "0x2222222222222222222222222222222222222222";
    let operation = "doWork";
    let params = json!([script_hash, operation, [], "ReadStates"]);

    let resp = methods
        .invoke_function(params)
        .await
        .expect("invoke_function should succeed");
    let obj = resp.as_object().unwrap();
    let script_hex = obj.get("script").and_then(|v| v.as_str()).unwrap();

    let hash = UInt160::from_str(script_hash).unwrap();
    let expected = build_expected_script(&hash, operation, &[], CallFlags::READ_STATES);
    assert_eq!(script_hex, expected);
}

#[tokio::test]
async fn test_invoke_function_invalid_call_flags() {
    let methods = setup_methods().await;
    let params = json!([
        "0x3333333333333333333333333333333333333333",
        "alpha",
        [],
        "BogusFlag"
    ]);

    let err = methods.invoke_function(params).await.unwrap_err();
    assert!(err
        .to_string()
        .to_lowercase()
        .contains("unsupported call flag"));
}

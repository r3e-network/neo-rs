use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_roundtrips_value() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let hash = UInt160::from_bytes(&[0x10u8; 20]).expect("hash");
    let contract = make_contract_state(100, hash, "StorageTest");
    let mut store = system.store_cache();
    store_contract_state(&mut store, &contract);
    store_storage_item(&mut store, contract.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [Value::String(hash.to_string()), Value::String(key_b64)];
    let result = (handler.callback())(&server, &params).expect("get storage");
    assert_eq!(
        result.as_str().unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_accepts_native_contract_name() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(&system.settings(), 0)
        .expect("gas state");

    let mut store = system.store_cache();
    store_contract_state(&mut store, &gas_state);
    store_storage_item(&mut store, gas_state.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [
        Value::String("GasToken".to_string()),
        Value::String(key_b64),
    ];
    let result = (handler.callback())(&server, &params).expect("get storage");
    assert_eq!(
        result.as_str().unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_accepts_native_contract_name() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(&system.settings(), 0)
        .expect("gas state");

    let mut store = system.store_cache();
    store_contract_state(&mut store, &gas_state);
    store_storage_item(&mut store, gas_state.id, &[0x01], &[0x02]);

    let key_b64 = BASE64_STANDARD.encode([0x01u8]);
    let params = [
        Value::String("GasToken".to_string()),
        Value::String(key_b64),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage");
    let obj = result.as_object().expect("object");
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    let first = results.first().and_then(Value::as_object).expect("entry");
    assert_eq!(
        first
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        BASE64_STANDARD.encode([0x02u8])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_rejects_unknown_contract_or_key() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x11u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::String(BASE64_STANDARD.encode([0x01u8])),
    ];
    let err = (handler.callback())(&server, &params).expect_err("unknown contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_contract().code());

    let hash = UInt160::from_bytes(&[0x12u8; 20]).expect("hash");
    let contract = make_contract_state(101, hash, "StorageTest2");
    let mut store = system.store_cache();
    store_contract_state(&mut store, &contract);

    let params = [
        Value::String(hash.to_string()),
        Value::String(BASE64_STANDARD.encode([0x01u8])),
    ];
    let err = (handler.callback())(&server, &params).expect_err("unknown storage item");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_storage_rejects_null_params() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getstorage");

    let params = [Value::Null, Value::String(BASE64_STANDARD.encode([0x01u8]))];
    let err = (handler.callback())(&server, &params).expect_err("null contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x13u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::Null,
    ];
    let err = (handler.callback())(&server, &params).expect_err("null key");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_queries_preserve_base64_error_messages() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let contract = UInt160::from_bytes(&[0x14u8; 20])
        .expect("hash")
        .to_string();

    let getstorage = find_handler(&handlers, "getstorage");
    let params = [
        Value::String(contract.clone()),
        Value::String("not-base64".to_string()),
    ];
    let err = (getstorage.callback())(&server, &params).expect_err("invalid storage key");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(
        rpc_error.data(),
        Some("invalid Base64 storage key: not-base64")
    );

    let findstorage = find_handler(&handlers, "findstorage");
    let params = [
        Value::String(contract),
        Value::String("not-base64".to_string()),
    ];
    let err = (findstorage.callback())(&server, &params).expect_err("invalid storage prefix");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(
        rpc_error.data(),
        Some("invalid Base64 storage prefix: not-base64")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_paginates_results() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let config = RpcServerConfig {
        find_storage_page_size: 10,
        ..Default::default()
    };
    let server = RpcServer::new(system.clone(), config);
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let hash = UInt160::from_bytes(&[0x20u8; 20]).expect("hash");
    let contract = make_contract_state(200, hash, "FindStorage");
    let mut store = system.store_cache();
    store_contract_state(&mut store, &contract);

    let page_size = server.settings().find_storage_page_size;
    let total_items = page_size + 5;
    for i in 0..total_items {
        let key = vec![0xAA, i as u8];
        let value = vec![i as u8];
        store.add(
            StorageKey::new(contract.id, key),
            StorageItem::from_bytes(value),
        );
    }
    store.try_commit().expect("commit test store");

    let prefix = BASE64_STANDARD.encode([0xAAu8]);
    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page1");
    let obj = result.as_object().expect("object");
    assert!(
        obj.get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
    assert_eq!(
        obj.get("results")
            .and_then(Value::as_array)
            .map(|v| v.len())
            .unwrap_or_default(),
        page_size
    );
    let next = obj.get("next").and_then(Value::as_u64).unwrap_or_default() as usize;
    assert_eq!(next, page_size);

    let params = [
        Value::String(hash.to_string()),
        Value::String(BASE64_STANDARD.encode([0xAAu8])),
        Value::Number((next as u64).into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page2");
    let obj = result.as_object().expect("object");
    assert!(
        !obj.get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    );
    assert_eq!(
        obj.get("results")
            .and_then(Value::as_array)
            .map(|v| v.len())
            .unwrap_or_default(),
        5
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_returns_empty_page_at_end() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let hash = UInt160::from_bytes(&[0x21u8; 20]).expect("hash");
    let contract = make_contract_state(201, hash, "FindStorageEnd");
    let mut store = system.store_cache();
    store_contract_state(&mut store, &contract);

    let prefix = [0xBBu8];
    for i in 0..3u8 {
        let key = vec![prefix[0], i];
        let value = vec![i];
        store.add(
            StorageKey::new(contract.id, key),
            StorageItem::from_bytes(value),
        );
    }
    store.try_commit().expect("commit test store");

    let prefix_b64 = BASE64_STANDARD.encode(prefix);
    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix_b64.clone()),
        Value::Number(0u64.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page1");
    let obj = result.as_object().expect("object");
    assert!(
        !obj.get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    );
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results.len(), 3);
    let next = obj.get("next").and_then(Value::as_u64).unwrap_or_default();
    assert_eq!(next, 3);

    let params = [
        Value::String(hash.to_string()),
        Value::String(prefix_b64),
        Value::Number(next.into()),
    ];
    let result = (handler.callback())(&server, &params).expect("find storage page2");
    let obj = result.as_object().expect("object");
    assert!(
        !obj.get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    );
    let results = obj
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert!(results.is_empty());
    let next_end = obj.get("next").and_then(Value::as_u64).unwrap_or_default();
    assert_eq!(next_end, next);
}

#[tokio::test(flavor = "multi_thread")]
async fn find_storage_rejects_null_params() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "findstorage");

    let params = [
        Value::Null,
        Value::String(BASE64_STANDARD.encode([0x01u8])),
        Value::Number(0u64.into()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("null contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [
        Value::String(
            UInt160::from_bytes(&[0x30u8; 20])
                .expect("hash")
                .to_string(),
        ),
        Value::Null,
    ];
    let err = (handler.callback())(&server, &params).expect_err("null prefix");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

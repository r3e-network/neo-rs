use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_sessions_disabled() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let err = (traverse.callback())(&server, &[]).expect_err("sessions disabled");
    assert_eq!(err.code(), -601);
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_session_rejects_sessions_disabled() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let terminate = find_handler(&handlers, "terminatesession");

    let err = (terminate.callback())(&server, &[]).expect_err("sessions disabled");
    assert_eq!(err.code(), -601);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_unknown_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let params = [
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(1)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("unknown session");
    assert_eq!(err.code(), -107);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_expired_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        session_expiration_time: 0,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let session = Session::new(
        server.system(), // Arc<NodeContext> coerced to Arc<dyn StoreProvider>
        server.system(), // Arc<NodeContext> coerced to Arc<dyn ConfigProvider>
        server.system().native_contract_provider(),
        vec![OpCode::RET.byte()],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");
    let session_id = server.store_session(session);

    let params = [
        Value::String(session_id.to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(1)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("expired session");
    assert_eq!(err.code(), -107);
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_session_returns_false_for_unknown_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let terminate = find_handler(&handlers, "terminatesession");

    let params = [Value::String(uuid::Uuid::new_v4().to_string())];
    let result = (terminate.callback())(&server, &params).expect("unknown session");
    assert_eq!(result, Value::Bool(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_count_limit_exceeded() {
    let config = RpcServerConfig {
        session_enabled: true,
        max_iterator_result_items: 1,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");

    let params = [
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::String(uuid::Uuid::new_v4().to_string()),
        Value::Number(serde_json::Number::from(2)),
    ];
    let err = (traverse.callback())(&server, &params).expect_err("count limit");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_rejects_invalid_count_params_with_stable_messages() {
    let config = RpcServerConfig {
        session_enabled: true,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");
    let session_id = Value::String(uuid::Uuid::new_v4().to_string());
    let iterator_id = Value::String(uuid::Uuid::new_v4().to_string());

    for count in [
        Value::String("1".to_string()),
        Value::Number(serde_json::Number::from(-1)),
        Value::Number(serde_json::Number::from(u64::from(u32::MAX) + 1)),
    ] {
        let params = [session_id.clone(), iterator_id.clone(), count];
        let err = (traverse.callback())(&server, &params).expect_err("invalid count");
        assert_invalid_params_data(&err, "traverseiterator expects integer parameter 3");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn traverse_iterator_returns_items_and_can_terminate_session() {
    let config = RpcServerConfig {
        session_enabled: true,
        max_iterator_result_items: 10,
        ..Default::default()
    };
    let server = make_server(config);
    let handlers = RpcServerSmartContract::register_handlers();
    let traverse = find_handler(&handlers, "traverseiterator");
    let terminate = find_handler(&handlers, "terminatesession");

    let session = Session::new(
        server.system(), // Arc<NodeContext> coerced to Arc<dyn StoreProvider>
        server.system(), // Arc<NodeContext> coerced to Arc<dyn ConfigProvider>
        server.system().native_contract_provider(),
        vec![OpCode::RET.byte()],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let entries = vec![
        (
            StorageKey::new(1, vec![0x01]),
            StorageItem::from_bytes(vec![0xaa]),
        ),
        (
            StorageKey::new(1, vec![0x02]),
            StorageItem::from_bytes(vec![0xbb]),
        ),
    ];
    let iterator = StorageIterator::new(entries, 0, FindOptions::None);
    let iterator_id = {
        let mut engine = session.engine_mut();
        engine
            .store_storage_iterator(iterator)
            .expect("store iterator")
    };
    let interop = Arc::new(IteratorInterop::new(iterator_id)) as Arc<dyn VmInteropInterface>;
    let iterator_uuid = session
        .register_iterator_interface(&interop)
        .expect("iterator uuid");

    let session_id = server.store_session(session);
    let params = [
        Value::String(session_id.to_string()),
        Value::String(iterator_uuid.to_string()),
        Value::Number(serde_json::Number::from(10)),
    ];
    let result = (traverse.callback())(&server, &params).expect("traverse result");

    let items = result.as_array().expect("array");
    assert_eq!(items.len(), 2);
    for (index, expected_key, expected_value) in [
        (0usize, vec![0x01u8], vec![0xaau8]),
        (1usize, vec![0x02u8], vec![0xbbu8]),
    ] {
        let entry = items[index].as_object().expect("entry object");
        assert_eq!(entry.get("type").and_then(Value::as_str), Some("Struct"));
        let values = entry
            .get("value")
            .and_then(Value::as_array)
            .expect("value array");
        let key_bytes = values
            .first()
            .and_then(|item| item.get("value"))
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 key"))
            .expect("key bytes");
        let value_bytes = values
            .get(1)
            .and_then(|item| item.get("value"))
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 value"))
            .expect("value bytes");
        assert_eq!(key_bytes, expected_key);
        assert_eq!(value_bytes, expected_value);
    }

    let tail = (traverse.callback())(&server, &params).expect("traverse tail");
    assert!(tail.as_array().expect("array").is_empty());

    let terminate_result =
        (terminate.callback())(&server, &[Value::String(session_id.to_string())])
            .expect("terminate session");
    assert_eq!(terminate_result, Value::Bool(true));

    let err = (traverse.callback())(&server, &params).expect_err("unknown session");
    assert_eq!(err.code(), -107);
}

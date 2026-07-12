use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_invalid_base64() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::String("not_base64".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid base64");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_invalid_block_bytes() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let payload = BASE64_STANDARD.encode([0u8; 4]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid block bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid block")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_accepts_valid_block() {
    let validator = KeyPair::from_private_key(&[0x10u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.store_cache();
    let account = validator.script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        1,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    // State-dependent transaction verification lives in the mempool admission
    // path; the handler below exercises block submission over that service seam.
    let store = system.store_cache();
    let block = build_signed_block(&settings, &store, &validator, vec![tx]);
    let expected_hash = Block::hash(&block);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("submit block");
    let hash = result.get("hash").and_then(Value::as_str).expect("hash");
    assert_eq!(hash, expected_hash.to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_already_exists() {
    let validator = KeyPair::from_private_key(&[0x11u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.store_cache();
    let account = validator.script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        2,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_index(0);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already exists");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_block() {
    let validator = KeyPair::from_private_key(&[0x12u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.store_cache();
    let account = validator.script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        3,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.witness = Witness::new();

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid block");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_prev_hash() {
    let validator = KeyPair::from_private_key(&[0x13u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.store_cache();
    let account = validator.script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        4,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_prev_hash(UInt256::from([0xABu8; 32]));

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid prev hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_reports_invalid_index() {
    let validator = KeyPair::from_private_key(&[0x14u8; 32]).expect("validator key");
    let settings = single_validator_settings(&validator);
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let mut store = system.store_cache();
    let account = validator.script_hash();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_custom(
        &settings,
        &validator,
        5,
        1_0000_0000,
        1_0000_0000,
        vec![OpCode::PUSH1.byte()],
    );
    let store = system.store_cache();
    let mut block = build_signed_block(&settings, &store, &validator, vec![tx]);
    block.header.set_index(block.header.index() + 10);

    let payload = BASE64_STANDARD.encode(block.to_array().expect("serialize block"));
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid index");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_null_input() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_block_rejects_empty_input() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "submitblock");

    let params = [Value::String(String::new())];
    let err = (handler.callback())(&server, &params).expect_err("empty input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

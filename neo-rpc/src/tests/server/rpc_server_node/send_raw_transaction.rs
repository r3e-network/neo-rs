use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_null_input() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_empty_input() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::String(String::new())];
    let err = (handler.callback())(&server, &params).expect_err("empty input");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_invalid_base64() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let params = [Value::String("not_base64".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid base64");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_rejects_invalid_transaction_bytes() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let payload = BASE64_STANDARD.encode([0u8; 4]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid transaction")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_accepts_valid_transaction() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction(&settings, &keypair, 1, 0);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let result =
        tokio::task::block_in_place(|| (handler.callback())(&server, &params)).expect("send raw");
    let hash = result.get("hash").and_then(Value::as_str).expect("hash");
    assert_eq!(hash, tx.hash().to_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_insufficient_funds() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x66u8; 32]).expect("keypair");
    let tx = build_signed_transaction(&settings, &keypair, 3, 0);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];

    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("insufficient funds");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::insufficient_funds().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_signature() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x77u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let mut tx = build_signed_transaction(&settings, &keypair, 4, 0);
    let mut witnesses = tx.witnesses().to_vec();
    if let Some(witness) = witnesses.get_mut(0) {
        if let Some(last) = witness.invocation_script.last_mut() {
            *last ^= 0x01;
        }
    }
    tx.set_witnesses(witnesses);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid signature");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_signature().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_size() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x88u8; 32]).expect("keypair");
    let mut tx = Transaction::new();
    tx.set_nonce(13);
    tx.set_network_fee(0);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_signers(vec![Signer::new(
        keypair.script_hash(),
        WitnessScope::GLOBAL,
    )]);
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(1, OracleResponseCode::Success, vec![0u8; MAX_RESULT_SIZE]),
    )]);
    tx.set_script(vec![OpCode::PUSH0.byte(); u16::MAX as usize]);
    tx.set_witnesses(vec![Witness::empty()]);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid size");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_size().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_script() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair");
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        8,
        0,
        1_0000_0000,
        1,
        vec![0xff],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid script");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_script().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_invalid_attribute() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let attributes = vec![TransactionAttribute::not_valid_before(5)];
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        9,
        0,
        1_0000_0000,
        1,
        vec![OpCode::PUSH1.byte()],
        attributes,
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("invalid attribute");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_attribute().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_expired_transaction() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair");
    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        10,
        0,
        1_0000_0000,
        0,
        vec![OpCode::PUSH1.byte()],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("expired transaction");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::expired_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_policy_failed() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let policy = PolicyContract::new();
    let mut store = system.store_cache();
    let key = StorageKey::create_with_uint160(policy.id(), 15, &account);
    store.add(key, StorageItem::from_bytes(Vec::new()));
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction_with(
        &settings,
        &keypair,
        11,
        0,
        1_0000_0000,
        1,
        vec![OpCode::PUSH1.byte()],
        Vec::new(),
    );
    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("policy failed");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::policy_failed().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_already_in_pool() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.try_commit().expect("commit test store");

    let tx = build_signed_transaction(&settings, &keypair, 12, 0);
    let mempool = system.mempool();
    let result = mempool.try_add(tx.clone(), store.data_cache());
    assert_eq!(result, VerifyResult::Succeed);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already in pool");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_in_pool().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_raw_transaction_reports_already_exists() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "sendrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let tx = build_signed_transaction(&settings, &keypair, 2, 0);
    let mut store = system.store_cache();
    persist_transaction_record(&mut store, &tx, 1);

    let payload = BASE64_STANDARD.encode(tx.to_bytes());
    let params = [Value::String(payload)];
    let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect_err("already exists");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());
    assert_eq!(rpc_error.message(), RpcError::already_exists().message());
    // C# GetRelayResult attaches WithData(reason.ToString()) to every error case.
    assert_eq!(rpc_error.data(), Some("AlreadyExists"));
}

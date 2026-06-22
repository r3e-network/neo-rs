use super::*;

#[test]
fn cancel_transaction_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "canceltransaction");
    let txid = UInt256::from([0x11u8; 32]).to_string();
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [
        Value::String(txid),
        Value::Array(vec![Value::String(address)]),
    ];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_txid() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String("invalid_txid".to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid txid");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_empty_signers() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x22u8; 32]).to_string()),
        Value::Array(Vec::new()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("empty signers");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::bad_request().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().store_cache();
    mint_gas(
        &mut store,
        &server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let txid = UInt256::from([0x33u8; 32]).to_string();
    let params = [
        Value::String(txid),
        Value::Array(vec![Value::String(address.clone())]),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let obj = result.as_object().expect("tx json");
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(address.as_str())
    );
    let signers = obj
        .get("signers")
        .and_then(Value::as_array)
        .expect("signers");
    let signer = signers[0].as_object().expect("signer");
    assert_eq!(signer.get("scopes").and_then(Value::as_str), Some("None"));
    let attributes = obj
        .get("attributes")
        .and_then(Value::as_array)
        .expect("attributes");
    assert_eq!(
        attributes[0].get("type").and_then(Value::as_str),
        Some("Conflicts")
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_signer_entry() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x66u8; 32]).to_string()),
        json!([{"account": "not-an-address"}]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid signer entry");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_confirmed_transaction() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let confirmed = build_signed_transaction_custom(
        &server.system().settings(),
        &keypair,
        7,
        0,
        1,
        vec![OpCode::PUSH1.byte()],
    );
    let mut store = server.system().store_cache();
    persist_transaction_record(&mut store, &confirmed);

    let params = [
        Value::String(confirmed.hash().to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let err = (handler.callback())(&server, &params).expect_err("confirmed tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::already_exists().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_invalid_extra_fee() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().store_cache();
    mint_gas(
        &mut store,
        &server.system().settings(),
        wallet_helper::to_script_hash(&address, server.system().settings().address_version)
            .expect("script hash"),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x44u8; 32]).to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("0".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid extra fee");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_rejects_wallet_fee_limit() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().store_cache();
    mint_gas(
        &mut store,
        &server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(UInt256::from([0x77u8; 32]).to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("100".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("wallet fee limit");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::wallet_fee_limit().code());
    assert!(rpc_error.data().is_some());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_applies_extra_fee() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1_000_000_000);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().store_cache();
    mint_gas(
        &mut store,
        &server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let txid = UInt256::from([0x55u8; 32]);
    let conflict = TransactionAttribute::Conflicts(Conflicts::new(txid));
    let signers = vec![Signer::new(keypair.get_script_hash(), WitnessScope::NONE)];
    let snapshot = server.system().store_cache();
    let snapshot_arc = Arc::new(snapshot.data_cache().clone());
    let base_tx = crate::server::wallet_compat::make_transaction(
        server.wallet().expect("wallet").as_ref(),
        snapshot_arc.as_ref(),
        &server.system().settings(),
        &[OpCode::RET.byte()],
        Some(signers[0].account),
        &signers,
        std::slice::from_ref(&conflict),
        server.settings().max_gas_invoke,
    )
    .expect("base cancel tx");
    let base_fee = base_tx.network_fee();

    let params = [
        Value::String(txid.to_string()),
        Value::Array(vec![Value::String(address)]),
        Value::String("1".to_string()),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let net_fee = result
        .get("netfee")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("netfee");
    let expected_extra = 10_i64.pow(
        8u32, /* GAS decimals (C# NativeContract.GAS.Decimals) */
    );
    assert_eq!(net_fee, base_fee + expected_extra);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_transaction_bumps_fee_for_mempool_conflict() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let server = make_authenticated_server_with_max_fee(1_000_000_000);
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "canceltransaction");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = server.system().store_cache();
    mint_gas(
        &mut store,
        &server.system().settings(),
        keypair.get_script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let conflict_tx = build_signed_transaction_custom(
        &server.system().settings(),
        &keypair,
        1,
        0,
        200_000_000,
        vec![OpCode::PUSH1.byte()],
    );
    let txid = conflict_tx.hash();
    let store_cache = server.system().store_cache();
    let verify = server
        .system()
        .mempool()
        .try_add(conflict_tx.clone(), store_cache.data_cache());
    assert_eq!(verify, VerifyResult::Succeed);

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(txid.to_string()),
        Value::Array(vec![Value::String(address)]),
    ];
    let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
        .expect("canceltransaction");
    let net_fee = result
        .get("netfee")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .expect("netfee");
    assert_eq!(net_fee, conflict_tx.network_fee() + 1);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

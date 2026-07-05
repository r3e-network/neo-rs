use super::*;

#[test]
fn send_many_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "sendmany");
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
    let outputs = json!([{
         "asset": asset,
         "value": "1",
         "address": address.clone()
    }]);
    let params = [Value::String(address), outputs];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_invalid_from() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    let outputs = json!([{
         "asset": asset,
         "value": "1",
         "address": wallet_helper::to_address(
             &UInt160::zero(),
             server.system().settings().address_version,
         )
    }]);
    let params = [Value::String("NotAnAddress".to_string()), outputs];

    let err = (handler.callback())(&server, &params).expect_err("invalid from");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_empty_outputs() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [Value::String(address), Value::Array(vec![])];
    let err = (handler.callback())(&server, &params).expect_err("empty outputs");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Argument 'to' can't be empty"),
        "unexpected error message: {:?}",
        rpc_error.data()
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_invalid_outputs_type() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let params = [
        Value::String(address),
        Value::String("not-an-array".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("invalid outputs");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert!(
        rpc_error
            .data()
            .unwrap_or_default()
            .contains("Invalid 'to' parameter"),
        "unexpected error message: {:?}",
        rpc_error.data()
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_rejects_non_positive_amount() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset_id = GasToken::script_hash();
    let asset = asset_id.to_string();
    for amount in ["-1", "0"] {
        let outputs = json!([{
             "asset": asset.clone(),
             "value": amount,
             "address": address.clone()
        }]);
        let params = [Value::String(address.clone()), outputs];
        let err = (handler.callback())(&server, &params).expect_err("invalid amount");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .unwrap_or_default()
                .contains(&format!("Amount of '{}' can't be negative.", asset_id)),
            "unexpected error message: {:?}",
            rpc_error.data()
        );
    }

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &system.settings(),
        keypair.script_hash(),
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    let outputs = json!([{
         "asset": asset,
         "value": "1",
         "address": address.clone()
    }]);
    let params = [Value::String(address.clone()), outputs];
    let result =
        tokio::task::block_in_place(|| (handler.callback())(&server, &params)).expect("sendmany");
    let obj = result.as_object().expect("tx json");
    assert_eq!(obj.len(), 12);
    assert_eq!(
        obj.get("sender").and_then(Value::as_str),
        Some(address.as_str())
    );

    let signers = obj
        .get("signers")
        .and_then(Value::as_array)
        .expect("signers");
    assert_eq!(signers.len(), 1);
    let signer = signers[0].as_object().expect("signer");
    let expected_account = keypair.script_hash().to_string();
    assert_eq!(
        signer.get("account").and_then(Value::as_str),
        Some(expected_account.as_str())
    );
    assert_eq!(
        signer.get("scopes").and_then(Value::as_str),
        Some("CalledByEntry")
    );

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_many_reports_invalid_operation_on_insufficient_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendmany");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let outputs = json!([{
         "asset": GasToken::script_hash().to_string(),
         "value": "100000000000000000",
         "address": address.clone()
    }]);
    let params = [Value::String(address), outputs];
    let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
    assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

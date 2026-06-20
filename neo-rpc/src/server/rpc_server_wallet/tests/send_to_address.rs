use super::*;

#[test]
fn send_to_address_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "sendtoaddress");
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_invalid_asset_id() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [
        Value::String("NotAnAssetId".to_string()),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("invalid asset");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_invalid_to_address() {
    let password = "rpc-pass";
    let (path, _keypair, _address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String("NotAnAddress".to_string()),
        Value::String("1".to_string()),
    ];

    let err = (handler.callback())(&server, &params).expect_err("invalid address");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_rejects_non_positive_amount() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    for amount in ["-1", "0"] {
        let params = [
            Value::String(asset.clone()),
            Value::String(address.clone()),
            Value::String(amount.to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid amount");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_address_reports_invalid_operation_on_insufficient_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let handler = find_handler(&handlers, "sendtoaddress");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address),
        Value::String("100000000000000000".to_string()),
    ];
    let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
    assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

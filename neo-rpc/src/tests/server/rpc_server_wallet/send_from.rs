use super::*;

#[test]
fn wallet_methods_require_open_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
    let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
    let wif = keypair.to_wif();

    let cases = vec![
        ("dumpprivkey", vec![Value::String(address.clone())]),
        ("getnewaddress", vec![]),
        ("getwalletbalance", vec![Value::String(asset.clone())]),
        ("getwalletunclaimedgas", vec![]),
        ("importprivkey", vec![Value::String(wif.clone())]),
        ("listaddress", vec![]),
    ];

    for (name, params) in cases {
        let handler = find_handler(&handlers, name);
        let err = (handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::no_opened_wallet().code(),
            "{} should require a wallet",
            name
        );
    }
}

#[test]
fn send_from_requires_wallet() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let send_handler = find_handler(&handlers, "sendfrom");
    let address =
        wallet_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address),
        Value::String("1".to_string()),
    ];

    let err = (send_handler.callback())(&server, &params).expect_err("no wallet");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_from_returns_transaction_json() {
    let password = "rpc-pass";
    let (path, keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let send_handler = find_handler(&handlers, "sendfrom");
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
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address.clone()),
        Value::String("1".to_string()),
    ];
    let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
        .expect("sendfrom");
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
async fn send_from_returns_invalid_request_without_funds() {
    let password = "rpc-pass";
    let (path, _keypair, address) = create_wallet_file(password).await;
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, authenticated_config());
    let handlers = RpcServerWallet::register_handlers();
    let open_handler = find_handler(&handlers, "openwallet");
    let send_handler = find_handler(&handlers, "sendfrom");
    let close_handler = find_handler(&handlers, "closewallet");

    let params = [
        Value::String(path.clone()),
        Value::String(password.to_string()),
    ];
    (open_handler.callback())(&server, &params).expect("open wallet");

    let asset = GasToken::script_hash().to_string();
    let params = [
        Value::String(asset),
        Value::String(address.clone()),
        Value::String(address.clone()),
        Value::String("1".to_string()),
    ];
    let err = (send_handler.callback())(&server, &params).expect_err("insufficient funds");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_request().code());

    let result = (close_handler.callback())(&server, &[]).expect("close wallet");
    assert_eq!(result.as_bool(), Some(true));

    fs::remove_file(path).ok();
}

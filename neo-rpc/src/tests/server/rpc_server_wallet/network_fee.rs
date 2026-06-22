use super::*;

#[test]
fn calculate_network_fee_requires_payload() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let err = (handler.callback())(&server, &[]).expect_err("missing payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[test]
fn await_wallet_future_supports_current_thread_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let result = runtime.block_on(async {
        RpcServerWallet::await_wallet_future(Box::pin(async { Ok::<i32, WalletError>(7) }))
    });

    assert_eq!(result.expect("await_wallet_future result"), 7);
}

#[test]
fn calculate_network_fee_returns_network_fee() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let settings = ProtocolSettings::default();
    let keypair = KeyPair::from_private_key(&[0x66u8; 32]).expect("keypair");
    let tx =
        build_signed_transaction_custom(&settings, &keypair, 1, 0, 0, vec![OpCode::PUSH1.byte()]);
    let payload = BASE64_STANDARD.encode(tx.to_bytes());

    let params = [Value::String(payload)];
    let result = (handler.callback())(&server, &params).expect("network fee");
    let obj = result.as_object().expect("network fee object");
    let fee = obj
        .get("networkfee")
        .and_then(Value::as_str)
        .expect("network fee");
    assert!(fee.parse::<i64>().is_ok());
}

#[test]
fn calculate_network_fee_rejects_invalid_payload() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");
    let params = [Value::String("invalid_base64".to_string())];

    let err = (handler.callback())(&server, &params).expect_err("invalid payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[test]
fn calculate_network_fee_rejects_invalid_transaction_bytes() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();
    let handler = find_handler(&handlers, "calculatenetworkfee");

    let payload = BASE64_STANDARD.encode([0x01u8, 0x02, 0x03]);
    let params = [Value::String(payload)];
    let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

use super::*;

#[test]
fn wallet_no_param_methods_reject_unexpected_parameters() {
    let server = make_authenticated_server();
    let handlers = RpcServerWallet::register_handlers();

    for method in [
        "closewallet",
        "getnewaddress",
        "listaddress",
        "getwalletunclaimedgas",
    ] {
        let handler = find_handler(&handlers, method);
        let error = (handler.callback())(&server, &[Value::from(1_u64)])
            .expect_err("wallet no-param method should reject parameters");
        let rpc_error: RpcError = error.into();

        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        let expected = format!("{method} expects no parameters");
        assert_eq!(rpc_error.data(), Some(expected.as_str()));
    }
}

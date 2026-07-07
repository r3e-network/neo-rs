use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn blockchain_no_param_methods_reject_unexpected_parameters() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();

    for method in [
        "getbestblockhash",
        "getblockcount",
        "getblockheadercount",
        "getnativecontracts",
        "getnextblockvalidators",
        "getcandidates",
        "getcommittee",
    ] {
        let handler = find_handler(&handlers, method);
        let error = (handler.callback())(&server, &[Value::from(1_u64)])
            .expect_err("blockchain no-param method should reject parameters");
        let rpc_error: RpcError = error.into();

        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .is_some_and(|data| data.contains(&format!("{method} expects no parameters"))),
            "unexpected error data: {:?}",
            rpc_error.data()
        );
    }
}

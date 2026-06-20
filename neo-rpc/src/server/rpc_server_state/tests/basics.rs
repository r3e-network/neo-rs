use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn state_handlers_error_when_service_not_registered() {
    let server = make_server_without_state();
    for method in [
        "getstateheight",
        "getstateroot",
        "getproof",
        "getstate",
        "findstates",
    ] {
        let params = [json!(0)];
        let err = call(&server, method, &params).expect_err("service missing");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::internal_server_error().code(),
            "{method} should fail without a registered state store"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn state_height_reports_null_then_index() {
    let (_system, state_store, server) = make_server_with_state();

    let result = call(&server, "getstateheight", &[]).expect("getstateheight");
    assert_eq!(result.get("localrootindex"), Some(&Value::Null));
    assert_eq!(result.get("validatedrootindex"), Some(&Value::Null));

    seed_state_root(&state_store, 7, 0xAB);
    let result = call(&server, "getstateheight", &[]).expect("getstateheight");
    assert_eq!(result.get("localrootindex"), Some(&json!(7)));
    assert_eq!(result.get("validatedrootindex"), Some(&json!(7)));
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_returns_committed_root() {
    let (_system, state_store, server) = make_server_with_state();
    let root = seed_state_root(&state_store, 3, 0xCD);

    let result = call(&server, "getstateroot", &[json!(3)]).expect("getstateroot");
    assert_eq!(result.get("index"), Some(&json!(3)));
    assert_eq!(
        result.get("roothash").and_then(Value::as_str),
        Some(root.root_hash.to_string().as_str())
    );
    assert_eq!(
        result
            .get("witnesses")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_rejects_unknown_index() {
    let (_system, _state_store, server) = make_server_with_state();
    let err = call(&server, "getstateroot", &[json!(42)]).expect_err("unknown root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_state_root().code());
}

#[test]
fn state_uint_parsers_preserve_csharp_binder_errors() {
    let err = RpcServerState::parse_uint256(&[], 0, "getproof").expect_err("missing UInt256");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(
        rpc_error.data(),
        Some("getproof expects UInt256 parameter at index 0")
    );

    let err = RpcServerState::parse_uint160(&[], 1, "getproof").expect_err("missing UInt160");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(
        rpc_error.data(),
        Some("getproof expects UInt160 parameter at index 1")
    );

    let err = RpcServerState::parse_uint256(&[json!("not-a-hash")], 0, "getproof")
        .expect_err("invalid UInt256");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(rpc_error.data(), Some("failed to parse UInt256 parameter"));

    let err = RpcServerState::parse_uint160(&[json!("not-a-hash")], 0, "getproof")
        .expect_err("invalid UInt160");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(rpc_error.data(), Some("failed to parse UInt160 parameter"));
}

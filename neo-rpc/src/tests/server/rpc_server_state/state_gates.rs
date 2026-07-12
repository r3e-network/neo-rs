use super::mpt_fixture::{b64, decode_b64_value, make_server_with_mpt};
use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_gate_on_current_root_without_full_state() {
    let fixture = make_server_with_mpt(false);
    let contract = json!(fixture.contract_hash.to_string());
    let key = json!(b64(&[0x0A, 0x01]));

    // The current root stays queryable.
    let current = call(
        &fixture.server,
        "getstate",
        &[
            json!(fixture.root2.to_string()),
            contract.clone(),
            key.clone(),
        ],
    )
    .expect("current root passes the gate");
    assert_eq!(decode_b64_value(&current), b"alpha-v2".to_vec());

    // Historical roots are rejected with UnsupportedState (C# CheckRootHash).
    for method in ["getproof", "getstate", "findstates"] {
        let err = call(
            &fixture.server,
            method,
            &[
                json!(fixture.root1.to_string()),
                contract.clone(),
                key.clone(),
            ],
        )
        .expect_err("historical root must be rejected without FullState");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unsupported_state().code(),
            "{method} must gate on the current root"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_state_data_uses_csharp_bool_casing() {
    // C# interpolates `bool.ToString()` into the -606 data string, so
    // the flag must read `True`/`False`, not Rust's `true`/`false`.
    let fixture = make_server_with_mpt(false);
    let err = call(
        &fixture.server,
        "getstate",
        &[
            json!(fixture.root1.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A, 0x01])),
        ],
    )
    .expect_err("historical root rejected without FullState");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unsupported_state().code());
    assert_eq!(
        rpc_error.data(),
        Some(
            format!(
                "fullState:False,current:{},rootHash:{}",
                fixture.root2, fixture.root1
            )
            .as_str()
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pruning_keeps_historical_root_metadata_queryable() {
    let fixture = make_server_with_mpt(false);

    let historical = call(&fixture.server, "getstateroot", &[json!(1)])
        .expect("historical root metadata remains available");
    assert_eq!(historical.get("index"), Some(&json!(1)));
    assert_eq!(
        historical.get("roothash").and_then(Value::as_str),
        Some(fixture.root1.to_string().as_str())
    );

    let height = call(&fixture.server, "getstateheight", &[]).expect("latest state height");
    assert_eq!(height.get("localrootindex"), Some(&json!(2)));
    assert_eq!(height.get("validatedrootindex"), Some(&json!(2)));
}

use super::mpt_fixture::{b64, decode_b64_value, fixture_storage_key, make_server_with_mpt};
use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn get_proof_round_trips_through_verify_proof() {
    let fixture = make_server_with_mpt(true);
    let params = [
        json!(fixture.root2.to_string()),
        json!(fixture.contract_hash.to_string()),
        json!(b64(&[0x0A, 0x03])),
    ];
    let proof = call(&fixture.server, "getproof", &params).expect("getproof");

    // The payload leads with the full storage key (C# WriteVarBytes(skey)).
    let payload = decode_b64_value(&proof);
    let (key, nodes) = RpcServerState::decode_proof_payload(&payload).expect("payload decodes");
    assert_eq!(key, fixture_storage_key(&[0x0A, 0x03]));
    assert!(!nodes.is_empty());

    // And verifies against the root it was issued for.
    let verify_params = [json!(fixture.root2.to_string()), proof];
    let value = call(&fixture.server, "verifyproof", &verify_params).expect("verifyproof");
    assert_eq!(decode_b64_value(&value), b"gamma".to_vec());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_state_serves_current_and_historical_roots() {
    let fixture = make_server_with_mpt(true);
    let contract = json!(fixture.contract_hash.to_string());
    let key = json!(b64(&[0x0A, 0x01]));

    let current = call(
        &fixture.server,
        "getstate",
        &[
            json!(fixture.root2.to_string()),
            contract.clone(),
            key.clone(),
        ],
    )
    .expect("getstate at current root");
    assert_eq!(decode_b64_value(&current), b"alpha-v2".to_vec());

    let historical = call(
        &fixture.server,
        "getstate",
        &[json!(fixture.root1.to_string()), contract.clone(), key],
    )
    .expect("getstate at historical root");
    assert_eq!(decode_b64_value(&historical), b"alpha".to_vec());

    // 0x0A04 only exists from block 2 onwards.
    let err = call(
        &fixture.server,
        "getstate",
        &[
            json!(fixture.root1.to_string()),
            contract,
            json!(b64(&[0x0A, 0x04])),
        ],
    )
    .expect_err("key absent under the historical root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_reject_unknown_contract() {
    let fixture = make_server_with_mpt(true);
    let foreign = neo_primitives::UInt160::from_bytes(&[0x99u8; 20]).expect("hash");
    for method in ["getproof", "getstate", "findstates"] {
        let err = call(
            &fixture.server,
            method,
            &[
                json!(fixture.root2.to_string()),
                json!(foreign.to_string()),
                json!(b64(&[0x0A])),
            ],
        )
        .expect_err("contract not deployed");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unknown_contract().code(),
            "{method} must report UnknownContract"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_proof_missing_key_reports_unknown_storage_item() {
    let fixture = make_server_with_mpt(true);
    let err = call(
        &fixture.server,
        "getproof",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A, 0x7F])),
        ],
    )
    .expect_err("storage item absent");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_storage_item().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn state_queries_report_resolution_failure_for_unknown_root() {
    let fixture = make_server_with_mpt(true);
    let unknown_root = neo_primitives::UInt256::from([0x77u8; 32]);
    let err = call(
        &fixture.server,
        "getstate",
        &[
            json!(unknown_root.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A, 0x01])),
        ],
    )
    .expect_err("root never persisted");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
}

#[test]
fn state_handlers_depend_on_provider_capabilities_not_mpt_mechanics() {
    let handlers = [
        include_str!("../../../server/rpc_server_state/proof.rs"),
        include_str!("../../../server/rpc_server_state/roots.rs"),
        include_str!("../../../server/rpc_server_state/state_queries.rs"),
        include_str!("../../../server/rpc_server_state/support.rs"),
    ]
    .join("\n");

    assert!(handlers.contains("state_provider_factory"));
    for forbidden in [
        "use neo_state_service::mpt_store",
        "MptReadSnapshot",
        ".open_trie(",
        "Self::mpt_store(",
        "Trie::<",
    ] {
        assert!(
            !handlers.contains(forbidden),
            "RPC state handlers must not bypass StateProviderFactory with {forbidden}"
        );
    }
}

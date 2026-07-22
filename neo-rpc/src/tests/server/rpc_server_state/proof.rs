use super::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_trie::Trie;

#[tokio::test(flavor = "multi_thread")]
async fn proof_handlers_report_unsupported_state() {
    let (_system, _state_store, server) = make_server_with_state();
    let root = neo_primitives::UInt256::from([0x11u8; 32]).to_string();
    let contract = neo_primitives::UInt160::zero().to_string();
    let key = BASE64_STANDARD.encode([0u8; 4]);

    for (method, params) in [
        (
            "getproof",
            vec![
                json!(root.clone()),
                json!(contract.clone()),
                json!(key.clone()),
            ],
        ),
        (
            "getstate",
            vec![
                json!(root.clone()),
                json!(contract.clone()),
                json!(key.clone()),
            ],
        ),
        ("findstates", vec![json!(root), json!(contract), json!(key)]),
    ] {
        let err = call(&server, method, &params).expect_err("proofs unsupported");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::unsupported_state().code(),
            "{method} should report the missing MPT backend"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_round_trips_value() {
    // Build a real MPT, capture a proof, and replay it through the
    // verifyproof handler — the same payload format the C# state
    // service emits.
    let store = Arc::new(MemoryMptStore::default());
    let mut trie = Trie::new(Arc::clone(&store), None, true);
    let key = [0x01u8, 0x02, 0x03, 0x04];
    let value = b"state-value".to_vec();
    trie.put(&key, &value).expect("trie put");
    let root_hash = trie.root_hash().expect("root hash");
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof present");

    let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
    let payload = RpcServerState::encode_proof_payload(&key, &nodes).unwrap();
    let payload_b64 = BASE64_STANDARD.encode(payload);

    let (_system, _state_store, server) = make_server_with_state();
    let params = [json!(root_hash.to_string()), json!(payload_b64)];
    let result = call(&server, "verifyproof", &params).expect("verifyproof");
    let decoded = BASE64_STANDARD
        .decode(result.as_str().expect("base64 value"))
        .expect("decode value");
    assert_eq!(decoded, value);
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_rejects_invalid_payload() {
    let (_system, _state_store, server) = make_server_with_state();
    let root = neo_primitives::UInt256::from([0x22u8; 32]).to_string();
    let bogus = BASE64_STANDARD.encode([0xFFu8; 2]);
    let err =
        call(&server, "verifyproof", &[json!(root), json!(bogus)]).expect_err("invalid payload");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_proof_rejects_wrong_root() {
    let store = Arc::new(MemoryMptStore::default());
    let mut trie = Trie::new(Arc::clone(&store), None, true);
    let key = [0x09u8, 0x08];
    trie.put(&key, b"value").expect("trie put");
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof present");
    let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
    let payload = RpcServerState::encode_proof_payload(&key, &nodes).unwrap();
    let payload_b64 = BASE64_STANDARD.encode(payload);

    let (_system, _state_store, server) = make_server_with_state();
    let wrong_root = neo_primitives::UInt256::from([0x33u8; 32]).to_string();
    let err = call(
        &server,
        "verifyproof",
        &[json!(wrong_root), json!(payload_b64)],
    )
    .expect_err("proof must not verify against a foreign root");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::verification_failed().code());
}

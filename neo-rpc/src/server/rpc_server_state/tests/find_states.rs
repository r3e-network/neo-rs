use super::mpt_fixture::{b64, decode_b64_value, make_server_with_mpt};
use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn find_states_returns_full_page_with_proofs() {
    let fixture = make_server_with_mpt(true);
    let result = call(
        &fixture.server,
        "findstates",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A])),
        ],
    )
    .expect("findstates");

    assert_eq!(result.get("truncated"), Some(&Value::Bool(false)));
    let results = result
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    let keys: Vec<Vec<u8>> = results
        .iter()
        .map(|entry| decode_b64_value(entry.get("key").expect("key")))
        .collect();
    let values: Vec<Vec<u8>> = results
        .iter()
        .map(|entry| decode_b64_value(entry.get("value").expect("value")))
        .collect();
    assert_eq!(
        keys,
        vec![vec![0x0A, 0x01], vec![0x0A, 0x03], vec![0x0A, 0x04],],
        "result keys must strip the contract id and come in trie order"
    );
    assert_eq!(
        values,
        vec![b"alpha-v2".to_vec(), b"gamma".to_vec(), b"delta".to_vec()]
    );

    // firstProof verifies to the first returned value.
    let first_proof = result
        .get("firstProof")
        .expect("firstProof present")
        .clone();
    let value = call(
        &fixture.server,
        "verifyproof",
        &[json!(fixture.root2.to_string()), first_proof],
    )
    .expect("first proof verifies");
    assert_eq!(decode_b64_value(&value), b"alpha-v2".to_vec());
    assert!(
        result.get("lastProof").is_some(),
        "lastProof for >1 results"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_truncates_and_resumes() {
    let fixture = make_server_with_mpt(true);
    let root = json!(fixture.root2.to_string());
    let contract = json!(fixture.contract_hash.to_string());
    let prefix = json!(b64(&[0x0A]));

    // Page 1: two of three entries -> truncated.
    let page1 = call(
        &fixture.server,
        "findstates",
        &[
            root.clone(),
            contract.clone(),
            prefix.clone(),
            Value::Null,
            json!(2),
        ],
    )
    .expect("findstates page 1");
    assert_eq!(page1.get("truncated"), Some(&Value::Bool(true)));
    let results1 = page1
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results1.len(), 2);
    assert!(page1.get("lastProof").is_some());

    // Page 2: resume strictly after the last returned key.
    let resume_key = results1[1].get("key").expect("resume key").clone();
    let page2 = call(
        &fixture.server,
        "findstates",
        &[root.clone(), contract.clone(), prefix.clone(), resume_key],
    )
    .expect("findstates page 2");
    assert_eq!(page2.get("truncated"), Some(&Value::Bool(false)));
    let results2 = page2
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results2.len(), 1);
    assert_eq!(
        decode_b64_value(results2[0].get("key").expect("key")),
        vec![0x0A, 0x04]
    );
    assert!(
        page2.get("firstProof").is_some(),
        "single result still proves first"
    );
    assert!(
        page2.get("lastProof").is_none(),
        "lastProof omitted for a single-entry page"
    );

    // A count that exactly matches the remaining entries is not truncated.
    let exact = call(
        &fixture.server,
        "findstates",
        &[root, contract, prefix, Value::Null, json!(3)],
    )
    .expect("findstates exact count");
    assert_eq!(exact.get("truncated"), Some(&Value::Bool(false)));
    assert_eq!(
        exact.get("results").and_then(Value::as_array).map(Vec::len),
        Some(3)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_rejects_from_key_outside_prefix() {
    let fixture = make_server_with_mpt(true);
    let err = call(
        &fixture.server,
        "findstates",
        &[
            json!(fixture.root2.to_string()),
            json!(fixture.contract_hash.to_string()),
            json!(b64(&[0x0A])),
            json!(b64(&[0x0B, 0x01])),
        ],
    )
    .expect_err("from key must extend the prefix");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_count_accepts_csharp_numeric_tokens() {
    // C# binds `count` through `ParameterConverter.ToNumeric<int>`:
    // JSON strings, float-integral numbers, and booleans all convert.
    let fixture = make_server_with_mpt(true);
    let root = json!(fixture.root2.to_string());
    let contract = json!(fixture.contract_hash.to_string());
    let prefix = json!(b64(&[0x0A]));

    let page_len = |count: Value| {
        let result = call(
            &fixture.server,
            "findstates",
            &[
                root.clone(),
                contract.clone(),
                prefix.clone(),
                Value::Null,
                count,
            ],
        )
        .expect("findstates accepts the count token");
        let len = result
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len)
            .expect("results array");
        let truncated = result
            .get("truncated")
            .and_then(Value::as_bool)
            .expect("truncated flag");
        (len, truncated)
    };

    // String-encoded integer (JString.AsNumber).
    assert_eq!(page_len(json!("2")), (2, true));
    // Whitespace is allowed by the C# invariant float parse.
    assert_eq!(page_len(json!(" 2 ")), (2, true));
    // Float-integral number.
    assert_eq!(page_len(json!(2.0)), (2, true));
    // String-encoded float-integral (exponent form allowed).
    assert_eq!(page_len(json!("3e0")), (3, false));
    // Boolean true converts to 1 (JBoolean.AsNumber).
    assert_eq!(page_len(json!(true)), (1, true));
    // Empty string converts to 0, i.e. the default page size.
    assert_eq!(page_len(json!("")), (3, false));
    // Negative counts select the default page size.
    assert_eq!(page_len(json!(-5)), (3, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn find_states_count_rejects_non_integral_tokens() {
    let fixture = make_server_with_mpt(true);
    let root = json!(fixture.root2.to_string());
    let contract = json!(fixture.contract_hash.to_string());
    let prefix = json!(b64(&[0x0A]));

    for (count, rendered) in [
        (json!(2.5), "2.5"),
        (json!("2.5"), "\"2.5\""),
        (json!("abc"), "\"abc\""),
        // Whitespace-only is not the empty string: the C# float parse
        // fails and yields NaN.
        (json!(" "), "\" \""),
        // Exceeds int.MaxValue.
        (json!(2_147_483_648i64), "2147483648"),
        (json!([1]), "[1]"),
    ] {
        let err = call(
            &fixture.server,
            "findstates",
            &[
                root.clone(),
                contract.clone(),
                prefix.clone(),
                Value::Null,
                count,
            ],
        )
        .expect_err("non-integral count tokens are rejected");
        let rpc_error: RpcError = err.into();
        assert_eq!(
            rpc_error.code(),
            RpcError::invalid_params().code(),
            "count {rendered} must be rejected"
        );
        assert_eq!(
            rpc_error.data(),
            Some(format!("Invalid System.Int32 value: {rendered}").as_str()),
            "count {rendered} must carry the C# data string"
        );
    }
}

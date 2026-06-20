use super::*;

#[tokio::test]
async fn get_block_count_matches_fixture() {
    let Some(fixture) = mock_no_param_fixture("getblockcountasync", "getblockcount").await else {
        return;
    };
    let expected = fixture
        .response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let actual = fixture.client.get_block_count().await.expect("block count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_hash_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getblockhashasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let index = params
        .get(0)
        .and_then(|value| value.as_number())
        .expect("block index") as u32;
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(r#""method"\s*:\s*"getblockhash".*"params"\s*:\s*\[\s*{index}\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_block_hash(index).await.expect("block hash");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_header_count_matches_fixture() {
    let Some(fixture) =
        mock_no_param_fixture("getblockheadercountasync", "getblockheadercount").await
    else {
        return;
    };
    let expected = fixture
        .response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let actual = fixture
        .client
        .get_block_header_count()
        .await
        .expect("block header count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_sys_fee_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getblocksysfeeasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let height = params
        .get(0)
        .and_then(|value| value.as_number())
        .expect("height") as u32;
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_token = response.get("result").expect("result token");
    let expected = if let Some(text) = expected_token.as_string() {
        BigInt::from_str(&text).expect("parse sysfee")
    } else if let Some(number) = expected_token.as_number() {
        BigInt::from(number as i64)
    } else {
        panic!("invalid sysfee token");
    };

    let mut server = Server::new_async().await;
    let body_re = format!(r#""method"\s*:\s*"getblocksysfee".*"params"\s*:\s*\[\s*{height}\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_block_sys_fee(height)
        .await
        .expect("block sys fee");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_header_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getblockheaderhexasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*{index}\s*\]"#),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#),
            )
        } else {
            panic!("invalid getblockheader param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let mut server = Server::new_async().await;
        let response_body = JToken::Object(response.clone()).to_string();
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(body_re))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let actual = client
            .get_block_header_hex(&hash_or_index)
            .await
            .expect("block header hex");
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn get_block_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getblockhexasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*{index}\s*\]"#),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#),
            )
        } else {
            panic!("invalid getblock param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let mut server = Server::new_async().await;
        let response_body = JToken::Object(response.clone()).to_string();
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(body_re))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let actual = client
            .get_block_hex(&hash_or_index)
            .await
            .expect("block hex");
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn get_raw_mempool_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getrawmempoolasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result")
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_string())
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getrawmempool".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_raw_mempool().await.expect("raw mempool");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_raw_mempool_both_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getrawmempoolbothasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_result = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");
    let expected = RpcRawMemPool::from_json(expected_result).expect("parse expected mempool");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getrawmempool".*"params"\s*:\s*\[\s*true\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_raw_mempool_both()
        .await
        .expect("raw mempool both");
    assert_eq!(actual.height, expected.height);
    assert_eq!(actual.verified, expected.verified);
    assert_eq!(actual.unverified, expected.unverified);
}

#[tokio::test]
async fn get_raw_transaction_hex_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getrawtransactionhexasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let escaped = escape(&hash);
    let body_re =
        format!(r#""method"\s*:\s*"getrawtransaction".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_raw_transaction_hex(&hash)
        .await
        .expect("raw transaction hex");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_nep17_balances_parses_rpc_payload() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let address = neo_primitives::UInt160::zero().to_address();
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"balance":[{{"assethash":"0x0000000000000000000000000000000000000000","amount":"5","lastupdatedblock":7}}],"address":"{address}"}}}}"#
    );

    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let balances = client
        .get_nep17_balances(&address)
        .await
        .expect("parse balances");
    assert_eq!(balances.user_script_hash, neo_primitives::UInt160::zero());
    assert_eq!(balances.balances.len(), 1);
    assert_eq!(balances.balances[0].last_updated_block, 7);
}

#[tokio::test]
async fn get_nep17_transfers_parses_rpc_payload() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    let address = neo_primitives::UInt160::zero().to_address();
    let tx_hash = neo_primitives::UInt256::zero();
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"address":"{address}","sent":[],"received":[{{"assethash":"0x0000000000000000000000000000000000000000","transferaddress":"{address}","amount":"1","blockindex":7,"transfernotifyindex":0,"timestamp":0,"txhash":"{tx_hash}"}}]}}}}"#
    );

    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let transfers = client
        .get_nep17_transfers(&address, None, None)
        .await
        .expect("parse transfers");
    assert_eq!(transfers.user_script_hash, neo_primitives::UInt160::zero());
    assert!(transfers.sent.is_empty());
    assert_eq!(transfers.received.len(), 1);
    assert_eq!(transfers.received[0].amount.to_string(), "1");
}

#[tokio::test]
async fn get_storage_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getstorageasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let hash_or_id = params.get(0).expect("hash or id");
        let key = params
            .get(1)
            .and_then(|value| value.as_string())
            .expect("key");
        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_string())
            .expect("result");

        let (body_re, hash_or_id) = if let Some(hash) = hash_or_id.as_string() {
            let escaped_hash = escape(&hash);
            let escaped_key = escape(&key);
            let body_re = format!(
                r#""method"\s*:\s*"getstorage".*"params"\s*:\s*\[\s*"{escaped_hash}"\s*,\s*"{escaped_key}"\s*\]"#
            );
            (body_re, hash.to_string())
        } else if let Some(id) = hash_or_id.as_number() {
            let id = id as i32;
            let id_string = id.to_string();
            let escaped_key = escape(&key);
            let body_re = format!(
                r#""method"\s*:\s*"getstorage".*"params"\s*:\s*\[\s*{id}\s*,\s*"{escaped_key}"\s*\]"#
            );
            (body_re, id_string)
        } else {
            panic!("invalid getstorage hash");
        };

        let mut server = Server::new_async().await;
        let response_body = JToken::Object(response.clone()).to_string();
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(body_re))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let actual = client
            .get_storage(&hash_or_id, &key)
            .await
            .expect("storage value");
        assert_eq!(actual, expected);
    }
}
